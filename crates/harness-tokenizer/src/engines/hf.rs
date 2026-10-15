//! Qwen3, DeepSeek V3 and GLM5 ordinary BPE counting on Hugging Face `tokenizers`.
//!
//! The bundled payload keeps the upstream normalizer, pre-tokenizer and BPE merge order. Added
//! tokens, the post-processor and the decoder are not part of the payload, so text that spells a
//! special token is split like any other text and no frame tokens are added.

use serde::Deserialize;
use tokenizers::models::bpe::{Vocab, BPE};
use tokenizers::{NormalizerWrapper, PreTokenizerWrapper, Tokenizer};

use super::pack::{decompress, Cursor};

#[derive(Deserialize)]
struct Header {
    normalizer: Option<NormalizerWrapper>,
    pre_tokenizer: Option<PreTokenizerWrapper>,
    ignore_merges: bool,
    vocab: usize,
    merges: usize,
    literals: usize,
}

pub(super) struct Hf(Tokenizer);

impl Hf {
    /// Payload layout: header JSON, literal vocabulary entries (the byte alphabet and extras),
    /// merge result ids as zigzag deltas, then 24-bit left and right part ids as byte planes.
    /// Every other vocabulary entry is the concatenation of its merge's parts.
    pub(super) fn load(packed: &[u8]) -> Option<Self> {
        let payload = decompress(packed)?;
        let mut cursor = Cursor::new(&payload);
        let header_len = usize::try_from(cursor.u32_le()?).ok()?;
        let header: Header = serde_json::from_slice(cursor.bytes(header_len)?).ok()?;

        let mut tokens: Vec<Option<String>> = vec![None; header.vocab];
        let mut ident = 0usize;
        for _ in 0..header.literals {
            ident = ident.checked_add(cursor.varint_len()?)?;
            let len = cursor.varint_len()?;
            let text = std::str::from_utf8(cursor.bytes(len)?).ok()?;
            *tokens.get_mut(ident)? = Some(text.to_owned());
        }

        let mut results = Vec::with_capacity(header.merges);
        let mut previous = 0i64;
        for _ in 0..header.merges {
            let zigzag = i64::try_from(cursor.varint()?).ok()?;
            previous += if zigzag & 1 == 0 {
                zigzag >> 1
            } else {
                -((zigzag + 1) >> 1)
            };
            results.push(usize::try_from(previous).ok()?);
        }
        let left = read_planes(&mut cursor, header.merges)?;
        let right = read_planes(&mut cursor, header.merges)?;
        if !cursor.is_empty() {
            return None;
        }

        let mut producer: Vec<Option<usize>> = vec![None; header.vocab];
        for (index, &result) in results.iter().enumerate() {
            let slot = producer.get_mut(result)?;
            slot.get_or_insert(index);
        }
        let mut pending = Vec::new();
        for id in 0..header.vocab {
            resolve(id, &mut tokens, &producer, &left, &right, &mut pending)?;
        }
        let tokens: Vec<String> = tokens.into_iter().collect::<Option<_>>()?;

        let mut merges = Vec::with_capacity(header.merges);
        for (&l, &r) in left.iter().zip(&right) {
            merges.push((tokens.get(l)?.clone(), tokens.get(r)?.clone()));
        }
        let mut vocab = Vocab::default();
        vocab.reserve(tokens.len());
        for (id, token) in tokens.into_iter().enumerate() {
            vocab.insert(token, u32::try_from(id).ok()?);
        }
        let model = BPE::builder()
            .vocab_and_merges(vocab, merges)
            .ignore_merges(header.ignore_merges)
            .build()
            .ok()?;

        let mut tokenizer = Tokenizer::new(model);
        tokenizer.with_normalizer(header.normalizer);
        tokenizer.with_pre_tokenizer(header.pre_tokenizer);
        Some(Self(tokenizer))
    }

    pub(super) fn count(&self, text: &str) -> Option<u32> {
        let encoding = self.0.encode_fast(text, false).ok()?;
        u32::try_from(encoding.len()).ok()
    }
}

fn read_planes(cursor: &mut Cursor<'_>, len: usize) -> Option<Vec<usize>> {
    let low = cursor.bytes(len)?;
    let mid = cursor.bytes(len)?;
    let high = cursor.bytes(len)?;
    Some(
        low.iter()
            .zip(mid)
            .zip(high)
            .map(|((&l, &m), &h)| usize::from(l) | usize::from(m) << 8 | usize::from(h) << 16)
            .collect(),
    )
}

/// Fill `tokens[id]` from its first producing merge, parts first. `pending` is the explicit
/// stack, so merge chains cannot overflow the call stack.
fn resolve(
    id: usize,
    tokens: &mut [Option<String>],
    producer: &[Option<usize>],
    left: &[usize],
    right: &[usize],
    pending: &mut Vec<usize>,
) -> Option<()> {
    pending.push(id);
    while let Some(&top) = pending.last() {
        if tokens.get(top)?.is_some() {
            pending.pop();
            continue;
        }
        let merge = (*producer.get(top)?)?;
        let (l, r) = (*left.get(merge)?, *right.get(merge)?);
        let (have_l, have_r) = (tokens.get(l)?.is_some(), tokens.get(r)?.is_some());
        if have_l && have_r {
            let joined = format!(
                "{}{}",
                tokens.get(l)?.as_deref()?,
                tokens.get(r)?.as_deref()?
            );
            *tokens.get_mut(top)? = Some(joined);
            pending.pop();
        } else {
            if !have_l {
                pending.push(l);
            }
            if !have_r {
                pending.push(r);
            }
            if pending.len() > tokens.len() {
                return None; // a cyclic payload would never terminate
            }
        }
    }
    Some(())
}
