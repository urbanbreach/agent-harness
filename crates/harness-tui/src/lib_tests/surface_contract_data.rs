#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParitySurface {
    StartupHome,
    LiveEmpty,
    LiveRun,
    CompletedPostRun,
    ReplayShell,
    OperatorSidebar,
    ReviewSurfaces,
    PermissionModal,
    CommandPalette,
    SlashOverlay,
    RuntimeStateOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellHierarchyContract {
    ComposeFirstHome,
    TranscriptFirstSession,
    OperatorSidebarSecondary,
    ReviewSecondary,
    InterruptiveOverlay,
    CommandOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChromeContract {
    FocusedStartupCard,
    QuietSessionShell,
    SecondaryPane,
    ReviewShell,
    ElevatedModal,
    ElevatedCommandOverlay,
    ElevatedRuntimeOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerContract {
    StartupPrimaryCallToAction,
    LiveProgressiveDisclosure,
    DisabledLiveProgressiveDisclosure,
    ReplayReadOnlyProgressiveDisclosure,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SidebarContract {
    PersistentWhenGeometryAllows,
    SecondaryOnly,
    SuppressedByOverlay,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SurfaceScopeContract {
    pub(super) surface: ParitySurface,
    pub(super) hierarchy: ShellHierarchyContract,
    pub(super) chrome: ChromeContract,
    pub(super) composer: ComposerContract,
    pub(super) sidebar: SidebarContract,
    pub(super) default_tab_chrome: bool,
    pub(super) debug_inspector_in_primary_path: bool,
}

pub(super) const FULL_SURFACE_SCOPE_MATRIX: [SurfaceScopeContract; 11] = [
    SurfaceScopeContract {
        surface: ParitySurface::StartupHome,
        hierarchy: ShellHierarchyContract::ComposeFirstHome,
        chrome: ChromeContract::FocusedStartupCard,
        composer: ComposerContract::StartupPrimaryCallToAction,
        sidebar: SidebarContract::NotApplicable,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::LiveEmpty,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::LiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::LiveRun,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::LiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::CompletedPostRun,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::DisabledLiveProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::ReplayShell,
        hierarchy: ShellHierarchyContract::TranscriptFirstSession,
        chrome: ChromeContract::QuietSessionShell,
        composer: ComposerContract::ReplayReadOnlyProgressiveDisclosure,
        sidebar: SidebarContract::PersistentWhenGeometryAllows,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::OperatorSidebar,
        hierarchy: ShellHierarchyContract::OperatorSidebarSecondary,
        chrome: ChromeContract::SecondaryPane,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SecondaryOnly,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::ReviewSurfaces,
        hierarchy: ShellHierarchyContract::ReviewSecondary,
        chrome: ChromeContract::ReviewShell,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::NotApplicable,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::PermissionModal,
        hierarchy: ShellHierarchyContract::InterruptiveOverlay,
        chrome: ChromeContract::ElevatedModal,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::CommandPalette,
        hierarchy: ShellHierarchyContract::CommandOverlay,
        chrome: ChromeContract::ElevatedCommandOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::SlashOverlay,
        hierarchy: ShellHierarchyContract::CommandOverlay,
        chrome: ChromeContract::ElevatedCommandOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
    SurfaceScopeContract {
        surface: ParitySurface::RuntimeStateOverlay,
        hierarchy: ShellHierarchyContract::InterruptiveOverlay,
        chrome: ChromeContract::ElevatedRuntimeOverlay,
        composer: ComposerContract::NotApplicable,
        sidebar: SidebarContract::SuppressedByOverlay,
        default_tab_chrome: false,
        debug_inspector_in_primary_path: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveMetadataHeadlineContract {
    Prohibited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LiveMetadataPlacementContract {
    StatusOrFooterOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HintDisclosureContract {
    ProgressiveBySpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerRowContract {
    NotPinnedToThreeRows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LiveShellNoiseBudgetContract {
    pub(super) dedicated_live_metadata_headline: LiveMetadataHeadlineContract,
    pub(super) live_metadata_placement: LiveMetadataPlacementContract,
    pub(super) hint_disclosure: HintDisclosureContract,
    pub(super) composer_rows: ComposerRowContract,
    pub(super) stable_shell_contexts: [ParitySurface; 9],
}

pub(super) const LIVE_SHELL_NOISE_BUDGET: LiveShellNoiseBudgetContract =
    LiveShellNoiseBudgetContract {
        dedicated_live_metadata_headline: LiveMetadataHeadlineContract::Prohibited,
        live_metadata_placement: LiveMetadataPlacementContract::StatusOrFooterOnly,
        hint_disclosure: HintDisclosureContract::ProgressiveBySpace,
        composer_rows: ComposerRowContract::NotPinnedToThreeRows,
        stable_shell_contexts: [
            ParitySurface::StartupHome,
            ParitySurface::LiveEmpty,
            ParitySurface::LiveRun,
            ParitySurface::CompletedPostRun,
            ParitySurface::ReplayShell,
            ParitySurface::PermissionModal,
            ParitySurface::CommandPalette,
            ParitySurface::SlashOverlay,
            ParitySurface::RuntimeStateOverlay,
        ],
    };
