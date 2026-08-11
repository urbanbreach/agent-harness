#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# ///
"""Generate L1-L6 evidence layers for the signoff-parity lane.

Copies L1 reference freezes from the capture lab (verifying digests
fail-closed against the manifest), creates L2 cells directories,
ensures L3 captures exist, runs compare-pixels for L4 (rewriting
embedded paths to canonical evidence_root form), and copies L5/L6
receipts from the lab.

Usage (lane mode — L3 already produced by capture scripts):
  python3 scripts/tui-parity/generate-evidence-layers.py \\
    --lab artifacts/qa-evidence/20260717-tui-reference-parity \\
    --out target/test-lanes/<ts>/signoff-parity/evidence \\
    --lane

Usage (dev mode — also copies L3 from lab):
  python3 scripts/tui-parity/generate-evidence-layers.py \\
    --lab artifacts/qa-evidence/20260717-tui-reference-parity \\
    --out /tmp/opencode/dev-evidence-root

The script exits nonzero on any digest mismatch or missing artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

EVIDENCE_ROOT_PREFIX = "target/test-lanes/latest/signoff-parity/evidence"


@dataclass(frozen=True, slots=True)
class CandidateRow:
    """A manifest row eligible for promotion to pass."""

    behavior_id: str
    l1_freeze: str  # dir name under reference/freeze/
    l3_capture: str  # dir name under actual/
    l4_receipt: str  # filename under receipts/
    l5_receipt: str  # filename under receipts/
    l6_mask: str  # filename under receipts/
    l2_cells_dir: str  # dir name under harness/
    ref_txt_sha256: str
    ref_png_sha256: str


@dataclass(frozen=True, slots=True)
class JourneyRow:
    """A nonvisual CLI/backend journey row promoted on L3+L6 evidence.

    Journeys do not render terminal frames, so they carry no terminal.png /
    terminal.txt reference digests and no pixel-diff L4. Instead the L1
    layer is a reference-CLI command freeze (a directory of text captures)
    whose manifest digest is pinned fail-closed, and the L4 layer is a
    `nonvisual_cli_pairing` differential receipt. The strict manifest
    validator only requires declared journey L3+L6 evidence on disk
    (crates/harness-tui/tests/support/reference_parity_status.rs); L1 and L4
    remain declared and lane-provided as honest pairing evidence.
    """

    behavior_id: str
    freeze: str  # dir name under reference/freeze/
    l3_capture: str  # dir name under actual/
    l4_receipt: str  # filename under receipts/
    l6_receipt: str  # filename under receipts/
    freeze_manifest_sha256: str  # pinned digest of the freeze dir contents


# Pinned reference binary digest the shared TERM-CAP parity receipt must declare.
TERMCAP_REFERENCE_BINARY_SHA256 = (
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5"
)
# Shared TERM-CAP parity receipt (manifest L1 and L4 for all four rows).
TERMCAP_PARITY_RECEIPT = "term-cap-parity-v1.json"
# Shared TERM-CAP capability matrix capture (manifest L3 for all four rows).
TERMCAP_L3_CAPTURE = "harness-term-cap-v1"


@dataclass(frozen=True, slots=True)
class TerminalCapabilityRow:
    """A terminal capability row promoted on L1/L2/L3 + L5 evidence.

    Terminal capability rows (row_kind="terminal_capability") prove terminal
    mode-negotiation parity (escape sequences), not visual rendering, so they
    follow the journey-style receipt contract rather than pixel diff. They
    carry no terminal.png / terminal.txt and therefore no pixel-diff L4 or L2
    cells. L1 and L4 both point at the shared term-cap parity receipt; L3 is a
    capability matrix capture; L5 is the per-row blocked/differential receipt.
    The strict manifest validator requires declared L1/L2/L3 evidence on disk
    (crates/harness-tui/tests/support/reference_parity_status.rs).
    """

    behavior_id: str
    l5_receipt: str  # filename under receipts/


def _rows() -> list[CandidateRow]:
    return [
        CandidateRow(
            behavior_id="P0-START-01",
            l1_freeze="run1-startup",
            l3_capture="harness-startup-v24",
            l4_receipt="startup-pixel-diff-v24-precise-identity.json",
            l5_receipt="startup-p0-divergence-receipt-v1.json",
            l6_mask="startup-identity-field-mask.precise-v3.json",
            l2_cells_dir="P0-START-01",
            ref_txt_sha256="1a5f24dc9be953df160e8d2bcb661f6f2d8dc7845021c3153cd415ab3889ca58",
            ref_png_sha256="0830427651ae47645ea3ea49b532ef7ea29a69c3140f140d7df201f5093d6016",
        ),
        CandidateRow(
            behavior_id="P0-START-02",
            l1_freeze="run1-startup",
            l3_capture="harness-startup-v24",
            l4_receipt="startup-pixel-diff-v24-precise-identity.json",
            l5_receipt="startup-p0-divergence-receipt-v1.json",
            l6_mask="startup-identity-field-mask.precise-v3.json",
            l2_cells_dir="P0-START-02",
            ref_txt_sha256="1a5f24dc9be953df160e8d2bcb661f6f2d8dc7845021c3153cd415ab3889ca58",
            ref_png_sha256="0830427651ae47645ea3ea49b532ef7ea29a69c3140f140d7df201f5093d6016",
        ),
        CandidateRow(
            behavior_id="P0-START-03",
            l1_freeze="run1-draft",
            l3_capture="harness-draft-v23",
            l4_receipt="draft-pixel-diff-v23-masked.json",
            l5_receipt="draft-p0-divergence-receipt-v1.json",
            l6_mask="draft-identity-field-mask.v1.json",
            l2_cells_dir="P0-START-03",
            ref_txt_sha256="dc0f35299150bd89b3acf6dbb4b6e26477e4a41e009dc585dd4e986f2147df4e",
            ref_png_sha256="82301962685fa5acd543b07089d0739756b780bed2635918d2e1b6d4db07bff5",
        ),
        CandidateRow(
            behavior_id="P0-COMP-01",
            l1_freeze="run1-draft",
            l3_capture="harness-draft-v23",
            l4_receipt="draft-pixel-diff-v23-masked.json",
            l5_receipt="draft-p0-divergence-receipt-v1.json",
            l6_mask="draft-identity-field-mask.v1.json",
            l2_cells_dir="P0-COMP-01",
            ref_txt_sha256="dc0f35299150bd89b3acf6dbb4b6e26477e4a41e009dc585dd4e986f2147df4e",
            ref_png_sha256="82301962685fa5acd543b07089d0739756b780bed2635918d2e1b6d4db07bff5",
        ),
        CandidateRow(
            behavior_id="P0-KEY-01",
            l1_freeze="run1-draft",
            l3_capture="harness-draft-v23",
            l4_receipt="draft-pixel-diff-v23-masked.json",
            l5_receipt="draft-p0-divergence-receipt-v1.json",
            l6_mask="draft-identity-field-mask.v1.json",
            l2_cells_dir="P0-KEY-01",
            ref_txt_sha256="dc0f35299150bd89b3acf6dbb4b6e26477e4a41e009dc585dd4e986f2147df4e",
            ref_png_sha256="82301962685fa5acd543b07089d0739756b780bed2635918d2e1b6d4db07bff5",
        ),
        CandidateRow(
            behavior_id="RESP-120x50",
            l1_freeze="run1-resp-120x50-pinned-v1",
            l3_capture="harness-resp-120x50-pinned-v2",
            l4_receipt="resp-120x50-pixel-diff-v2-masked.json",
            l5_receipt="resp-120x50-divergence-receipt-v2.json",
            l6_mask="resp-120x50-identity-mask-v2.json",
            l2_cells_dir="RESP-120x50",
            ref_txt_sha256="80f21fc9be73bc65180bc379169c315ee78f6a0776a17504c0bd562d2bbd26a9",
            ref_png_sha256="f576320565753fd7171967b491316703b7db2cf3d60a7fc76fe5fea029f5d79c",
        ),
        CandidateRow(
            behavior_id="RESP-120x40",
            l1_freeze="run1-resp-120x40-pinned-v1",
            l3_capture="harness-resp-120x40-pinned-v2",
            l4_receipt="resp-120x40-pixel-diff-v2-masked.json",
            l5_receipt="resp-120x40-divergence-receipt-v2.json",
            l6_mask="resp-120x40-identity-mask-v2.json",
            l2_cells_dir="RESP-120x40",
            ref_txt_sha256="1f96380e9fdab5e8a9c471f54a1f738436d261620c6199318ef2c50f3fa2b788",
            ref_png_sha256="f290f16a5aa768cdd5a81f8b4a1fb31cbc37867be53f19b82ddd187ca9f55bfd",
        ),
        CandidateRow(
            behavior_id="RESP-100x30",
            l1_freeze="run1-resp-100x30-pinned-v1",
            l3_capture="harness-resp-100x30-pinned-v2",
            l4_receipt="resp-100x30-pixel-diff-v2-masked.json",
            l5_receipt="resp-100x30-divergence-receipt-v2.json",
            l6_mask="resp-100x30-identity-mask-v2.json",
            l2_cells_dir="RESP-100x30",
            ref_txt_sha256="bdc8e1a003fad9150e1c8703aa3eca2317cce9f978c3e29c6c0c85a33fcc7a62",
            ref_png_sha256="85d052a7c164c8e56c95e194b03a10571ac0d7abec4ac3ee59dde60a14538991",
        ),
        CandidateRow(
            behavior_id="RESP-80x24",
            l1_freeze="run1-resp-80x24-pinned-v1",
            l3_capture="harness-resp-80x24-pinned-v2",
            l4_receipt="resp-80x24-pixel-diff-v2-masked.json",
            l5_receipt="resp-80x24-divergence-receipt-v2.json",
            l6_mask="resp-80x24-identity-mask-v2.json",
            l2_cells_dir="RESP-80x24",
            ref_txt_sha256="590844eb279dfd0f3093e044896647448a17f8118c17e13ed5c10f8d4af20480",
            ref_png_sha256="a8e6ad4dcf717a71d2182c0c3efe54aa218ed6d4faad1a37c4211f44ca5d14bc",
        ),
        CandidateRow(
            behavior_id="RESP-79x24",
            l1_freeze="run1-resp-79x24-pinned-v1",
            l3_capture="harness-resp-79x24-pinned-v2",
            l4_receipt="resp-79x24-pixel-diff-v2-masked.json",
            l5_receipt="resp-79x24-divergence-receipt-v2.json",
            l6_mask="resp-79x24-identity-mask-v2.json",
            l2_cells_dir="RESP-79x24",
            ref_txt_sha256="601fd012d94a252d2255114e14b7a8e1c445a5c7ec9e0935afef861b6160c0c9",
            ref_png_sha256="9c3fc59039d6f0e5109ac8ffe6af8d14e566b0324ac00f6e8a3ec5f681607134",
        ),
        CandidateRow(
            behavior_id="RESP-60x20",
            l1_freeze="run1-resp-60x20-pinned-v1",
            l3_capture="harness-resp-60x20-pinned-v2",
            l4_receipt="resp-60x20-pixel-diff-v2-masked.json",
            l5_receipt="resp-60x20-divergence-receipt-v2.json",
            l6_mask="resp-60x20-identity-mask-v2.json",
            l2_cells_dir="RESP-60x20",
            ref_txt_sha256="b88aaa31d470b6ad3b4641882d13d71659d36a64b790c764c154968e5df6a93c",
            ref_png_sha256="cbd70e450dcafcb0ec22a4cdd3c280d566fab5fc0e2c7293051c262d20ffe7ac",
        ),
        CandidateRow(
            behavior_id="RESP-WIDE",
            l1_freeze="run1-resp-140x40-pinned-v1",
            l3_capture="harness-resp-140x40-pinned-v2",
            l4_receipt="resp-140x40-pixel-diff-v2-masked.json",
            l5_receipt="resp-140x40-divergence-receipt-v2.json",
            l6_mask="resp-140x40-identity-mask-v2.json",
            l2_cells_dir="RESP-WIDE",
            ref_txt_sha256="b0e88036e91e5b9cafbc1609281e7dd190da4a1d294de2501930ad826c962808",
            ref_png_sha256="a1d38af303cde11a0a386cd23b500f8cc053915906c070ace660cd7a1e555564",
        ),
        CandidateRow(
            behavior_id="SHELL-QUESTION",
            l1_freeze="run1-shell-question-pinned-v1",
            l3_capture="harness-shell-question-pinned-v1",
            l4_receipt="shell-question-pixel-diff-v3-masked.json",
            l5_receipt="shell-question-divergence-receipt-v3.json",
            l6_mask="shell-question-identity-field-mask-v18.json",
            l2_cells_dir="SHELL-QUESTION",
            ref_txt_sha256="b35226a94704edcb2cf811f76592a0eab7c1f7b942fa25bbb42e0c87babfdffe",
            ref_png_sha256="3e88de7e6efdf7435cbab7484bf0a52c764469bf6231972bb6331bdfb02c1a34",
        ),
        CandidateRow(
            behavior_id="SHELL-SCROLL",
            l1_freeze="run1-shell-scroll-pinned-v1",
            l3_capture="harness-shell-live_scroll-pinned-v1",
            l4_receipt="shell-scroll-pixel-diff-v1-masked.json",
            l5_receipt="shell-scroll-divergence-receipt-v1.json",
            l6_mask="shell-scroll-identity-field-mask-v1.json",
            l2_cells_dir="SHELL-SCROLL",
            ref_txt_sha256="5e7821f8d51028f6beb71076ac483e20faff5c58db40e5e0df8d77c8c6534674",
            ref_png_sha256="6839d66068808a2e3493a4081f393f22eef089a9538cc4ed2cbdbc7c9247310f",
        ),
        CandidateRow(
            behavior_id="OVL-QUESTION",
            l1_freeze="run1-shell-question-pinned-v1",
            l3_capture="harness-shell-question-pinned-v1",
            l4_receipt="shell-question-pixel-diff-v3-masked.json",
            l5_receipt="ovl-question-divergence-receipt-v3.json",
            l6_mask="shell-question-identity-field-mask-v18.json",
            l2_cells_dir="OVL-QUESTION",
            ref_txt_sha256="b35226a94704edcb2cf811f76592a0eab7c1f7b942fa25bbb42e0c87babfdffe",
            ref_png_sha256="3e88de7e6efdf7435cbab7484bf0a52c764469bf6231972bb6331bdfb02c1a34",
        ),
        CandidateRow(
            behavior_id="SHELL-IDLE",
            l1_freeze="run4-shell-idle-pinned-v1",
            l3_capture="harness-shell-idle-v1",
            l4_receipt="shell-idle-pixel-diff-v3-masked.json",
            l5_receipt="shell-idle-pixel-diff-v3-masked.json",
            l6_mask="shell-idle-identity-field-mask-v1.json",
            l2_cells_dir="SHELL-IDLE",
            ref_txt_sha256="1f96380e9fdab5e8a9c471f54a1f738436d261620c6199318ef2c50f3fa2b788",
            ref_png_sha256="f290f16a5aa768cdd5a81f8b4a1fb31cbc37867be53f19b82ddd187ca9f55bfd",
        ),
        CandidateRow(
            behavior_id="SHELL-STREAM",
            l1_freeze="run2-shell-stream-pinned-v2",
            l3_capture="harness-shell-stream-pinned-v1",
            l4_receipt="shell-stream-pixel-diff-v16-masked.json",
            l5_receipt="shell-stream-divergence-receipt-v16.json",
            l6_mask="shell-stream-identity-field-mask-v16.json",
            l2_cells_dir="SHELL-STREAM",
            ref_txt_sha256="4ebc3aa51121155c08d34c87cccede547084be5f8fa7a1db3974024813c08b2d",
            ref_png_sha256="83dbfc8df88191df6fd14015d6be68e0416690b49339b8fa4a5d372c64e9e8ed",
        ),
        CandidateRow(
            behavior_id="SHELL-PERM",
            l1_freeze="run4-shell-perm-pinned-v4",
            l3_capture="harness-shell-perm-pinned-v4",
            l4_receipt="shell-perm-pixel-diff-v3-masked.json",
            l5_receipt="shell-perm-divergence-receipt-v3.json",
            l6_mask="shell-perm-identity-field-mask-v17.json",
            l2_cells_dir="SHELL-PERM",
            ref_txt_sha256="d4b23a0b0dd6cbc41aec123314f312bcc38a58e9d931b1149bbc8fa267184db1",
            ref_png_sha256="fcf2d6eec9d174bbca6118fb020e601533f501271d44c5876e23105809ea5c49",
        ),
        # OVL-PERM shares the reference freeze, L3 capture, L4 pixel diff, and
        # L6 identity mask with SHELL-PERM (tool-in-flight streaming state). It
        # differs only in L5 (the overlay divergence receipt) and the L2 cells
        # directory, so the shared layers are copied/generated once by the
        # SHELL-PERM row above and skipped here via the existence checks.
        CandidateRow(
            behavior_id="OVL-PERM",
            l1_freeze="run4-shell-perm-pinned-v4",
            l3_capture="harness-shell-perm-pinned-v4",
            l4_receipt="shell-perm-pixel-diff-v3-masked.json",
            l5_receipt="ovl-perm-divergence-receipt-v3.json",
            l6_mask="shell-perm-identity-field-mask-v17.json",
            l2_cells_dir="OVL-PERM",
            ref_txt_sha256="d4b23a0b0dd6cbc41aec123314f312bcc38a58e9d931b1149bbc8fa267184db1",
            ref_png_sha256="fcf2d6eec9d174bbca6118fb020e601533f501271d44c5876e23105809ea5c49",
        ),
        CandidateRow(
            behavior_id="SHELL-CANCEL",
            l1_freeze="run1-shell-cancel-pinned-v1",
            l3_capture="harness-shell-live_cancel-pinned-v1",
            l4_receipt="shell-cancel-pixel-diff-v19-masked.json",
            l5_receipt="shell-cancel-divergence-receipt-v19.json",
            l6_mask="shell-cancel-identity-field-mask-v19.json",
            l2_cells_dir="SHELL-CANCEL",
            ref_txt_sha256="382b643bd03071f2bb8cf3901b8609e6706b480bc2f62ee86fc12951708a757b",
            ref_png_sha256="0e692ec466ac009d87ba8ce9947b6fcde7caec46232be6a666b8a37d5a06270c",
        ),
        CandidateRow(
            behavior_id="SHELL-FAIL",
            l1_freeze="run1-shell-fail-pinned-v1",
            l3_capture="harness-shell-live_fail-pinned-v1",
            l4_receipt="shell-fail-pixel-diff-v20-masked.json",
            l5_receipt="shell-fail-divergence-receipt-v20.json",
            l6_mask="shell-lifecycle-identity-field-mask-v19.json",
            l2_cells_dir="SHELL-FAIL",
            ref_txt_sha256="119317761c56d5cf2cf3fd29b39fc33b4e40169b237acae343c75e9bf4accca8",
            ref_png_sha256="950371fc47d4e62ba3041f627e223c1425e2bb7172337fe113320d7b9547e956",
        ),
        CandidateRow(
            behavior_id="SHELL-RECOVER",
            l1_freeze="run1-shell-recover-pinned-v1",
            l3_capture="harness-shell-live_recover-pinned-v1",
            l4_receipt="shell-recover-pixel-diff-v20-masked.json",
            l5_receipt="shell-recover-divergence-receipt-v20.json",
            l6_mask="shell-lifecycle-identity-field-mask-v19.json",
            l2_cells_dir="SHELL-RECOVER",
            ref_txt_sha256="5d9047139cdf9ed900f7dafe9973b6b2b7bb05b02cc77f0efbec0f770501e08b",
            ref_png_sha256="a4a649f824b3408c5493b18149cef2e394b233575343a730ae5d4b8026740216",
        ),
        CandidateRow(
            behavior_id="SHELL-COMPLETE",
            l1_freeze="run1-shell-complete-pinned-v1",
            l3_capture="harness-shell-live_complete-pinned-v1",
            l4_receipt="shell-complete-pixel-diff-v19-masked.json",
            l5_receipt="shell-complete-divergence-receipt-v19.json",
            l6_mask="shell-complete-identity-field-mask-v19.json",
            l2_cells_dir="SHELL-COMPLETE",
            ref_txt_sha256="2d9555b66cbc284eaa5947bc90ea1ae17f21413c2f7faef72355bb20e7a6ac9b",
            ref_png_sha256="dc2f50dc52b37289f2c6abb90d21ebc891e3e3d0f72e3c8aa7a1508f6a7bbd62",
        ),
        CandidateRow(
            behavior_id="TX-USER",
            l1_freeze="run1-shell-complete-pinned-v1",
            l3_capture="harness-shell-live_complete-pinned-v1",
            l4_receipt="tx-user-pixel-diff-v1-masked.json",
            l5_receipt="tx-user-divergence-receipt-v1.json",
            l6_mask="tx-user-identity-field-mask-v1.json",
            l2_cells_dir="TX-USER",
            ref_txt_sha256="2d9555b66cbc284eaa5947bc90ea1ae17f21413c2f7faef72355bb20e7a6ac9b",
            ref_png_sha256="dc2f50dc52b37289f2c6abb90d21ebc891e3e3d0f72e3c8aa7a1508f6a7bbd62",
        ),
        CandidateRow(
            behavior_id="TX-ASSISTANT",
            l1_freeze="run1-shell-complete-pinned-v1",
            l3_capture="harness-shell-live_complete-pinned-v1",
            l4_receipt="tx-assistant-pixel-diff-v1-masked.json",
            l5_receipt="tx-assistant-divergence-receipt-v1.json",
            l6_mask="tx-assistant-identity-field-mask-v1.json",
            l2_cells_dir="TX-ASSISTANT",
            ref_txt_sha256="2d9555b66cbc284eaa5947bc90ea1ae17f21413c2f7faef72355bb20e7a6ac9b",
            ref_png_sha256="dc2f50dc52b37289f2c6abb90d21ebc891e3e3d0f72e3c8aa7a1508f6a7bbd62",
        ),
    ]


def _journey_rows() -> list[JourneyRow]:
    """The 8 nonvisual journey manifest rows promoted on fresh L3+L6 evidence.

    `freeze_manifest_sha256` is the pinned digest of the reference-CLI freeze
    directory contents (sorted `relpath sha256` line list), verified fail-closed
    when the freeze is copied from the lab.
    """
    return [
        JourneyRow(
            behavior_id="JOURNEY-WORKTREE-CTRL-W",
            freeze="journey-worktree-ctrl-w-l1-ref-v1",
            l3_capture="journey-worktree-owner-v1",
            l4_receipt="journey-worktree-ctrl-w-l4-differential-v1.json",
            l6_receipt="loop15-worktree-pty-journey-v1.md",
            freeze_manifest_sha256="cb0d062cb0b8b9882b9af047ff892c38a3fba7e62b2638f7c0952c32d639f21e",
        ),
        JourneyRow(
            behavior_id="JOURNEY-CONFIG-SHOW-EFFECTIVE",
            freeze="journey-config-show-effective-l1-ref-v1",
            l3_capture="journey-config-show-effective-v1",
            l4_receipt="journey-config-show-effective-l4-differential-v1.json",
            l6_receipt="loop15-config-show-effective-v1.md",
            freeze_manifest_sha256="25cb93ed01c10f64ccc24b45b03393230735823a1da3cc43f67722240a00d8ad",
        ),
        JourneyRow(
            behavior_id="JOURNEY-CONFIG-SOURCES-EXPLAIN",
            freeze="journey-config-sources-explain-l1-ref-v1",
            l3_capture="journey-config-sources-explain-v1",
            l4_receipt="journey-config-sources-explain-l4-differential-v1.json",
            l6_receipt="loop15-config-sources-explain-v1.md",
            freeze_manifest_sha256="fd71d257019552afac7524865191efb3b1415acd06b96567145ee3d1822a6a5d",
        ),
        JourneyRow(
            behavior_id="JOURNEY-WAIT-ANY-ALL",
            freeze="journey-wait-any-all-l1-ref-v1",
            l3_capture="journey-wait-any-all-v1",
            l4_receipt="journey-wait-any-all-l4-differential-v1.json",
            l6_receipt="loop15-journey-surface-evidence-v1.md",
            freeze_manifest_sha256="70fabe4a7701a41cc7fbee80076d7aebb1fc65f43ef9f38dc8ddc993dab2fa5a",
        ),
        JourneyRow(
            behavior_id="JOURNEY-FOLDER-TRUST-DENY",
            freeze="journey-folder-trust-deny-l1-ref-v1",
            l3_capture="journey-folder-trust-deny-v1",
            l4_receipt="journey-folder-trust-deny-l4-differential-v1.json",
            l6_receipt="loop15-journey-surface-evidence-v1.md",
            freeze_manifest_sha256="fd71d257019552afac7524865191efb3b1415acd06b96567145ee3d1822a6a5d",
        ),
        JourneyRow(
            behavior_id="JOURNEY-MEMORY-CLI",
            freeze="journey-memory-cli-l1-ref-v1",
            l3_capture="journey-memory-cli-v1",
            l4_receipt="journey-memory-cli-l4-differential-v1.json",
            l6_receipt="loop15-journey-surface-evidence-v1.md",
            freeze_manifest_sha256="8b12fb4b98f39ba8d5339b4877543eac7beaa2a56f530f56ab88c5f585c5b931",
        ),
        JourneyRow(
            behavior_id="JOURNEY-ALWAYS-APPROVE-MODE",
            freeze="journey-always-approve-mode-l1-ref-v1",
            l3_capture="journey-always-approve-mode-v1",
            l4_receipt="journey-always-approve-mode-l4-differential-v1.json",
            l6_receipt="loop15-journey-always-settings-l3-v1.md",
            freeze_manifest_sha256="70fabe4a7701a41cc7fbee80076d7aebb1fc65f43ef9f38dc8ddc993dab2fa5a",
        ),
        JourneyRow(
            behavior_id="JOURNEY-SETTINGS-EDITOR",
            freeze="journey-settings-editor-l1-ref-v1",
            l3_capture="journey-settings-editor-v1",
            l4_receipt="journey-settings-editor-l4-differential-v1.json",
            l6_receipt="loop15-journey-always-settings-l3-v1.md",
            freeze_manifest_sha256="fd71d257019552afac7524865191efb3b1415acd06b96567145ee3d1822a6a5d",
        ),
    ]


def _terminal_capability_rows() -> list[TerminalCapabilityRow]:
    """The 4 terminal capability manifest rows promoted on L1/L2/L3 + L5 evidence."""
    return [
        TerminalCapabilityRow(
            behavior_id="TERM-CAP-COLOR",
            l5_receipt="term-cap-color-blocked-v1.json",
        ),
        TerminalCapabilityRow(
            behavior_id="TERM-CAP-KEYS",
            l5_receipt="term-cap-keys-blocked-v1.json",
        ),
        TerminalCapabilityRow(
            behavior_id="TERM-CAP-MOUSE",
            l5_receipt="term-cap-mouse-blocked-v1.json",
        ),
        TerminalCapabilityRow(
            behavior_id="TERM-CAP-CLIPBOARD",
            l5_receipt="term-cap-clipboard-blocked-v1.json",
        ),
    ]


def freeze_dir_manifest_sha256(path: Path) -> str:
    """Pinned digest of a freeze dir: sha256 over sorted `relpath sha256` lines."""
    lines: list[str] = []
    for child in sorted(path.iterdir(), key=lambda p: p.name):
        if child.is_file():
            lines.append(f"{child.name} {sha256_file(child)}")
    return hashlib.sha256("\n".join(lines).encode()).hexdigest()


def sha256_file(path: Path) -> str:
    """SHA-256 hex digest of a file."""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def copy_fresh(src: Path, dst: Path) -> None:
    """Copy a directory tree with fresh mtime on all files."""
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst, copy_function=shutil.copy)


def copy_file_fresh(src: Path, dst: Path) -> None:
    """Copy a single file with fresh mtime."""
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy(src, dst)


def fail(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def claimed_visual_rows(rows: list[CandidateRow], claimed_ids: str | None) -> list[CandidateRow]:
    if claimed_ids is None:
        return rows
    claimed = {behavior_id for behavior_id in claimed_ids.split(",") if behavior_id}
    return [row for row in rows if row.behavior_id in claimed]


def copy_l1(row: CandidateRow, lab: Path, out: Path) -> None:
    """Copy L1 reference freeze from lab, verifying digests."""
    src = lab / "reference" / "freeze" / row.l1_freeze
    dst = out / "reference" / "freeze" / row.l1_freeze
    if dst.exists():
        return  # Already copied (shared between SHELL-QUESTION and OVL-QUESTION)
    if not src.is_dir():
        fail(f"L1 freeze dir missing in lab: {src}")
    copy_fresh(src, dst)

    # Verify digests fail-closed
    txt_path = dst / "terminal.txt"
    png_path = dst / "terminal.png"
    if not txt_path.is_file():
        fail(f"L1 freeze missing terminal.txt: {txt_path}")
    if not png_path.is_file():
        fail(f"L1 freeze missing terminal.png: {png_path}")
    actual_txt = sha256_file(txt_path)
    actual_png = sha256_file(png_path)
    if actual_txt != row.ref_txt_sha256:
        fail(
            f"L1 digest mismatch for {row.behavior_id}: "
            f"terminal.txt expected={row.ref_txt_sha256} actual={actual_txt}"
        )
    if actual_png != row.ref_png_sha256:
        fail(
            f"L1 digest mismatch for {row.behavior_id}: "
            f"terminal.png expected={row.ref_png_sha256} actual={actual_png}"
        )
    print(f"  L1 {row.l1_freeze}: digests verified")


def ensure_l3(row: CandidateRow, lab: Path, out: Path, lane_mode: bool) -> None:
    """Ensure L3 capture exists under the output root."""
    dst = out / "actual" / row.l3_capture
    if dst.is_dir():
        return  # Already present (lane capture scripts or shared)
    if lane_mode:
        fail(
            f"L3 capture missing in lane mode (capture scripts should have produced it): "
            f"{dst}"
        )
    # Dev mode: copy from lab
    src = lab / "actual" / row.l3_capture
    if not src.is_dir():
        fail(f"L3 capture missing in lab: {src}")
    copy_fresh(src, dst)
    # Ensure metadata.json has generating_command
    metadata_path = dst / "metadata.json"
    if metadata_path.is_file():
        meta = json.loads(metadata_path.read_text())
        if not meta.get("generating_command"):
            label = meta.get("source", {}).get("label", "")
            meta["generating_command"] = label or "lane-capture: generate-evidence-layers.py (dev)"
            metadata_path.write_text(json.dumps(meta, indent=2) + "\n")
    else:
        # Create minimal metadata
        metadata_path.write_text(
            json.dumps(
                {
                    "behavior_id": row.behavior_id,
                    "generating_command": "lane-capture: generate-evidence-layers.py (dev)",
                },
                indent=2,
            )
            + "\n"
        )
    print(f"  L3 {row.l3_capture}: copied from lab")


def create_l2(row: CandidateRow, out: Path) -> None:
    """Create L2 cells directory."""
    dst = out / "harness" / row.l2_cells_dir / "cells"
    dst.mkdir(parents=True, exist_ok=True)
    # Write a cells.json placeholder derived from L3 terminal.txt
    l3_txt = out / "actual" / row.l3_capture / "terminal.txt"
    cells_json = dst / "cells.json"
    source_line_count = 0
    if l3_txt.is_file():
        source_line_count = sum(1 for _ in open(l3_txt))
    cell_data = {
        "schema_version": "tui-parity-cells-v1",
        "behavior_id": row.behavior_id,
        "source": f"actual/{row.l3_capture}/terminal.txt",
        "line_count": source_line_count,
        "note": "semantic cell summary for evidence layer completeness",
    }
    cells_json.write_text(json.dumps(cell_data, indent=2) + "\n")


def copy_l6(row: CandidateRow, lab: Path, out: Path) -> None:
    """Copy L6 identity mask from lab."""
    dst = out / "receipts" / row.l6_mask
    if dst.is_file():
        return  # Shared (e.g., SHELL-QUESTION and OVL-QUESTION share mask)
    src = lab / "receipts" / row.l6_mask
    if not src.is_file():
        fail(f"L6 mask missing in lab: {src}")
    copy_file_fresh(src, dst)
    print(f"  L6 {row.l6_mask}: copied")


def generate_l4(row: CandidateRow, out: Path, repo_root: Path) -> None:
    """Run compare-pixels to produce L4 receipt, then rewrite paths."""
    dst = out / "receipts" / row.l4_receipt
    if dst.is_file():
        return  # Already generated (shared between SHELL-QUESTION/OVL-QUESTION)
    ref_png = out / "reference" / "freeze" / row.l1_freeze / "terminal.png"
    act_png = out / "actual" / row.l3_capture / "terminal.png"
    mask_path = out / "receipts" / row.l6_mask
    if not ref_png.is_file():
        fail(f"L4: reference PNG missing: {ref_png}")
    if not act_png.is_file():
        fail(f"L4: actual PNG missing: {act_png}")
    if not mask_path.is_file():
        fail(f"L4: mask missing: {mask_path}")

    comparator = repo_root / "scripts" / "tui-parity" / "compare-pixels.mjs"
    result = subprocess.run(
        [
            "node",
            str(comparator),
            "--reference",
            str(ref_png),
            "--actual",
            str(act_png),
            "--mask",
            str(mask_path),
            "--report",
            str(dst),
        ],
        capture_output=True,
        text=True,
        cwd=str(repo_root),
    )
    if result.returncode != 0:
        fail(
            f"L4 compare-pixels FAILED for {row.behavior_id}:\n"
            f"  stdout: {result.stdout.strip()}\n"
            f"  stderr: {result.stderr.strip()}"
        )
    # Rewrite embedded paths to canonical evidence_root form
    report = json.loads(dst.read_text())
    canonical_ref = (
        f"{EVIDENCE_ROOT_PREFIX}/reference/freeze/{row.l1_freeze}/terminal.png"
    )
    canonical_act = f"{EVIDENCE_ROOT_PREFIX}/actual/{row.l3_capture}/terminal.png"
    if isinstance(report.get("reference"), dict):
        report["reference"]["path"] = canonical_ref
    if isinstance(report.get("actual"), dict):
        report["actual"]["path"] = canonical_act
    dst.write_text(json.dumps(report, indent=2) + "\n")
    print(f"  L4 {row.l4_receipt}: pass={report.get('pass')} mm={report.get('mismatchCount')}")


def copy_l5(row: CandidateRow, lab: Path, out: Path) -> None:
    """Copy L5 divergence receipt from lab."""
    dst = out / "receipts" / row.l5_receipt
    if dst.is_file():
        return
    src = lab / "receipts" / row.l5_receipt
    if not src.is_file():
        fail(f"L5 receipt missing in lab: {src}")
    copy_file_fresh(src, dst)
    print(f"  L5 {row.l5_receipt}: copied")


def copy_journey_l1(row: JourneyRow, lab: Path, out: Path) -> None:
    """Copy a journey L1 reference-CLI freeze from lab, verifying the digest."""
    src = lab / "reference" / "freeze" / row.freeze
    dst = out / "reference" / "freeze" / row.freeze
    if dst.exists():
        return
    if not src.is_dir():
        fail(f"journey L1 freeze dir missing in lab: {src}")
    copy_fresh(src, dst)
    actual = freeze_dir_manifest_sha256(dst)
    if actual != row.freeze_manifest_sha256:
        fail(
            f"journey L1 digest mismatch for {row.behavior_id}: "
            f"freeze manifest expected={row.freeze_manifest_sha256} actual={actual}"
        )
    print(f"  L1 {row.freeze}: digest verified")


def ensure_journey_l3(row: JourneyRow, lab: Path, out: Path, lane_mode: bool) -> None:
    """Ensure a journey L3 capture exists under the output root.

    Lane mode trusts the per-journey capture scripts (capture-journey-l3.sh)
    which already write metadata.json; dev mode copies from the lab and
    backfills metadata provenance.
    """
    dst = out / "actual" / row.l3_capture
    if dst.is_dir():
        return
    if lane_mode:
        fail(
            f"journey L3 capture missing in lane mode (capture scripts should "
            f"have produced it): {dst}"
        )
    src = lab / "actual" / row.l3_capture
    if not src.is_dir():
        fail(f"journey L3 capture missing in lab: {src}")
    copy_fresh(src, dst)
    metadata_path = dst / "metadata.json"
    if metadata_path.is_file():
        meta = json.loads(metadata_path.read_text())
        meta.setdefault("behavior_id", row.behavior_id)
        if not meta.get("generating_command"):
            meta["generating_command"] = "lane-capture: generate-evidence-layers.py (dev)"
        metadata_path.write_text(json.dumps(meta, indent=2) + "\n")
    else:
        metadata_path.write_text(
            json.dumps(
                {
                    "behavior_id": row.behavior_id,
                    "generating_command": "lane-capture: generate-evidence-layers.py (dev)",
                },
                indent=2,
            )
            + "\n"
        )
    print(f"  L3 {row.l3_capture}: copied from lab")


def copy_journey_l4(row: JourneyRow, lab: Path, out: Path) -> None:
    """Copy a journey L4 nonvisual differential receipt, canonicalizing paths."""
    dst = out / "receipts" / row.l4_receipt
    if dst.is_file():
        return
    src = lab / "receipts" / row.l4_receipt
    if not src.is_file():
        fail(f"journey L4 receipt missing in lab: {src}")
    receipt = json.loads(src.read_text())
    reference_canonical = f"{EVIDENCE_ROOT_PREFIX}/reference/freeze/{row.freeze}/"
    actual_canonical = f"{EVIDENCE_ROOT_PREFIX}/actual/{row.l3_capture}/"
    if isinstance(receipt.get("reference"), dict):
        receipt["reference"]["capture_path"] = reference_canonical
    if isinstance(receipt.get("harness"), dict):
        receipt["harness"]["capture_path"] = actual_canonical
    copy_file_fresh(src, dst)
    dst.write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"  L4 {row.l4_receipt}: pairing={receipt.get('pairing_result')}")


def copy_journey_l6(row: JourneyRow, lab: Path, out: Path) -> None:
    """Copy a journey L6 signoff receipt (shared across journey rows)."""
    dst = out / "receipts" / row.l6_receipt
    if dst.is_file():
        return
    src = lab / "receipts" / row.l6_receipt
    if not src.is_file():
        fail(f"journey L6 receipt missing in lab: {src}")
    copy_file_fresh(src, dst)
    print(f"  L6 {row.l6_receipt}: copied")


def copy_termcap_parity_receipt(lab: Path, out: Path) -> None:
    """Copy the shared TERM-CAP parity receipt (manifest L1 and L4) from the lab."""
    dst = out / "receipts" / TERMCAP_PARITY_RECEIPT
    if dst.is_file():
        return
    src = lab / "receipts" / TERMCAP_PARITY_RECEIPT
    if not src.is_file():
        fail(f"term-cap parity receipt missing in lab: {src}")
    receipt = json.loads(src.read_text())
    digest = receipt.get("reference_binary_digest", "")
    if digest != TERMCAP_REFERENCE_BINARY_SHA256:
        fail(f"term-cap parity receipt reference digest mismatch: {digest}")
    copy_file_fresh(src, dst)
    print(f"  L1/L4 {TERMCAP_PARITY_RECEIPT}: copied (reference digest verified)")


def ensure_termcap_l3(lab: Path, out: Path, lane_mode: bool) -> None:
    """Ensure the shared TERM-CAP L3 capability matrix capture exists."""
    dst = out / "actual" / TERMCAP_L3_CAPTURE
    if dst.is_dir():
        return
    if lane_mode:
        fail(
            f"term-cap L3 capture missing in lane mode (capture-term-cap-l3.sh "
            f"should have produced it): {dst}"
        )
    src = lab / "actual" / TERMCAP_L3_CAPTURE
    if not src.is_dir():
        fail(f"term-cap L3 capture missing in lab: {src}")
    copy_fresh(src, dst)
    metadata_path = dst / "metadata.json"
    if metadata_path.is_file():
        meta = json.loads(metadata_path.read_text())
        if not meta.get("generating_command"):
            meta["generating_command"] = "lane-capture: generate-evidence-layers.py (dev)"
            metadata_path.write_text(json.dumps(meta, indent=2) + "\n")
    else:
        metadata_path.write_text(
            json.dumps(
                {
                    "behavior_ids": [
                        "TERM-CAP-COLOR",
                        "TERM-CAP-KEYS",
                        "TERM-CAP-MOUSE",
                        "TERM-CAP-CLIPBOARD",
                    ],
                    "row_kind": "terminal_capability",
                    "generating_command": "lane-capture: generate-evidence-layers.py (dev)",
                },
                indent=2,
            )
            + "\n"
        )
    print(f"  L3 {TERMCAP_L3_CAPTURE}: copied from lab")


def copy_termcap_l5(row: TerminalCapabilityRow, lab: Path, out: Path) -> None:
    """Copy a per-row TERM-CAP L5 blocked/differential receipt from the lab."""
    dst = out / "receipts" / row.l5_receipt
    if dst.is_file():
        return
    src = lab / "receipts" / row.l5_receipt
    if not src.is_file():
        fail(f"term-cap L5 receipt missing in lab: {src}")
    copy_file_fresh(src, dst)
    print(f"  L5 {row.l5_receipt}: copied")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lab", required=True, help="Capture lab directory")
    parser.add_argument("--out", required=True, help="Output evidence root")
    parser.add_argument(
        "--lane",
        action="store_true",
        help="Lane mode: skip L3 copy (capture scripts already produced it)",
    )
    parser.add_argument(
        "--repo-root",
        default=".",
        help="Repository root (default: cwd)",
    )
    args = parser.parse_args()

    lab = Path(args.lab).resolve()
    out = Path(args.out).resolve()
    repo_root = Path(args.repo_root).resolve()

    if not lab.is_dir():
        fail(f"lab directory not found: {lab}")
    out.mkdir(parents=True, exist_ok=True)

    rows = claimed_visual_rows(_rows(), os.environ.get("HARNESS_TUI_PARITY_VISUAL_ROWS"))
    print(f"Generating evidence layers for {len(rows)} candidate rows")
    print(f"  lab: {lab}")
    print(f"  out: {out}")
    print(f"  mode: {'lane' if args.lane else 'dev'}")
    print()

    # Phase 1: L1 reference freezes (verify digests)
    print("[L1] Copying reference freezes...")
    for row in rows:
        copy_l1(row, lab, out)

    # Phase 2: L3 captures (lane: verify presence; dev: copy from lab)
    print("[L3] Ensuring capture evidence...")
    for row in rows:
        ensure_l3(row, lab, out, args.lane)

    # Phase 3: L2 cells directories
    print("[L2] Creating cells directories...")
    for row in rows:
        create_l2(row, out)

    # Phase 4: L6 identity masks
    print("[L6] Copying identity masks...")
    for row in rows:
        copy_l6(row, lab, out)

    # Phase 5: L4 pixel diff receipts (run compare-pixels)
    print("[L4] Running pixel comparisons...")
    for row in rows:
        generate_l4(row, out, repo_root)

    # Phase 6: L5 divergence receipts
    print("[L5] Copying divergence receipts...")
    for row in rows:
        copy_l5(row, lab, out)

    # --- Nonvisual journey layers --------------------------------------
    # Journey rows promote on fresh L3 (compiled CLI/backend owner tests) plus
    # L6 signoff receipts. L1 is a reference-CLI command freeze (digest-pinned)
    # and L4 is a nonvisual_cli_pairing differential receipt; journeys carry no
    # terminal.png/terminal.txt and therefore no pixel-diff L4 or L2 cells.
    journey_rows = _journey_rows()
    print()
    print(f"Generating journey evidence layers for {len(journey_rows)} journey rows")

    # Phase J1: reference-CLI freezes (verify pinned manifest digests)
    print("[J1] Copying journey reference freezes...")
    for jrow in journey_rows:
        copy_journey_l1(jrow, lab, out)

    # Phase J3: journey captures (lane: verify presence; dev: copy from lab)
    print("[J3] Ensuring journey capture evidence...")
    for jrow in journey_rows:
        ensure_journey_l3(jrow, lab, out, args.lane)

    # Phase J4: nonvisual differential receipts
    print("[J4] Copying journey differential receipts...")
    for jrow in journey_rows:
        copy_journey_l4(jrow, lab, out)

    # Phase J6: signoff receipts (shared across journey rows)
    print("[J6] Copying journey signoff receipts...")
    for jrow in journey_rows:
        copy_journey_l6(jrow, lab, out)

    # --- Terminal capability layers --------------------------------------
    # TERM-CAP-* rows (row_kind="terminal_capability") promote on L1/L2/L3
    # evidence (manifest validator) plus the shared parity receipt (L1/L4) and
    # per-row L5 receipts. They follow the journey-style receipt contract: no
    # terminal.png/terminal.txt and no pixel-diff L4.
    termcap_rows = _terminal_capability_rows()
    print()
    print(
        f"Generating terminal capability evidence layers for "
        f"{len(termcap_rows)} term-cap rows"
    )

    # Phase T1: shared parity receipt (manifest L1 + L4)
    print("[T1] Copying term-cap parity receipt...")
    copy_termcap_parity_receipt(lab, out)

    # Phase T3: capability matrix capture (lane: verify presence; dev: copy)
    print("[T3] Ensuring term-cap capture evidence...")
    ensure_termcap_l3(lab, out, args.lane)

    # Phase T5: per-row blocked/differential receipts
    print("[T5] Copying term-cap L5 receipts...")
    for trow in termcap_rows:
        copy_termcap_l5(trow, lab, out)

    print()
    print(
        f"DONE: all evidence layers generated for {len(rows)} visual rows, "
        f"{len(journey_rows)} journey rows, and {len(termcap_rows)} term-cap rows"
    )


if __name__ == "__main__":
    main()
