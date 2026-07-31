#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    Models,
    Tools,
    Memory,
    Appearance,
    Permissions,
    Features,
    Advanced,
}

impl Category {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Models => "Models & Providers",
            Self::Tools => "Tools & Search",
            Self::Memory => "Memory",
            Self::Appearance => "Appearance & Input",
            Self::Permissions => "Permissions & Safety",
            Self::Features => "Features",
            Self::Advanced => "Advanced",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::Models => "Default models, providers, discovery, and capabilities",
            Self::Tools => "Tool surface, web search, and provider credentials",
            Self::Memory => "Session compaction and long-term memories",
            Self::Appearance => "Theme, terminal behavior, input, and notifications",
            Self::Permissions => "Default permission profile and legacy safety controls",
            Self::Features => "Stable and beta capabilities reported by app-server",
            Self::Advanced => "Prompts, templates, experimental, and legacy settings",
        }
    }
}

pub(crate) const fn categories() -> &'static [Category] {
    &[
        Category::Models,
        Category::Tools,
        Category::Memory,
        Category::Appearance,
        Category::Permissions,
        Category::Features,
        Category::Advanced,
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subpage {
    Models,
    Search,
    SessionMemoryTemplates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingOption {
    pub(crate) label: &'static str,
    pub(crate) value: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingKind {
    Bool,
    Integer,
    Text,
    DefaultProvider,
    DefaultModel,
    Enum(&'static [SettingOption]),
    Theme,
    PermissionProfile,
    Subpage(Subpage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingDefinition {
    pub(crate) id: &'static str,
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) category: Category,
    pub(crate) kind: SettingKind,
    pub(crate) default: &'static str,
    pub(crate) takes_effect: &'static str,
}

pub(super) const EFFORT: &[SettingOption] = &[
    SettingOption {
        label: "None",
        value: "none",
    },
    SettingOption {
        label: "Minimal",
        value: "minimal",
    },
    SettingOption {
        label: "Low",
        value: "low",
    },
    SettingOption {
        label: "Medium",
        value: "medium",
    },
    SettingOption {
        label: "High",
        value: "high",
    },
    SettingOption {
        label: "Extra high",
        value: "xhigh",
    },
    SettingOption {
        label: "Max",
        value: "max",
    },
    SettingOption {
        label: "Ultra",
        value: "ultra",
    },
];

const TOOL_SURFACE: &[SettingOption] = &[
    SettingOption {
        label: "Claude",
        value: "claude",
    },
    SettingOption {
        label: "Codex",
        value: "codex",
    },
];

const WEB_SEARCH: &[SettingOption] = &[
    SettingOption {
        label: "Disabled",
        value: "disabled",
    },
    SettingOption {
        label: "Cached",
        value: "cached",
    },
    SettingOption {
        label: "Live",
        value: "live",
    },
];

const COMPACT_MEMORY: &[SettingOption] = &[
    SettingOption {
        label: "Off",
        value: "off",
    },
    SettingOption {
        label: "Enqueue",
        value: "enqueue",
    },
    SettingOption {
        label: "Blocking",
        value: "blocking",
    },
];

const ALT_SCREEN: &[SettingOption] = &[
    SettingOption {
        label: "Auto",
        value: "auto",
    },
    SettingOption {
        label: "Always",
        value: "always",
    },
    SettingOption {
        label: "Never",
        value: "never",
    },
];

const SESSION_PICKER: &[SettingOption] = &[
    SettingOption {
        label: "Comfortable",
        value: "comfortable",
    },
    SettingOption {
        label: "Dense",
        value: "dense",
    },
];

const NOTIFICATION_METHOD: &[SettingOption] = &[
    SettingOption {
        label: "Automatic",
        value: "auto",
    },
    SettingOption {
        label: "OSC 9",
        value: "osc9",
    },
    SettingOption {
        label: "Terminal bell",
        value: "bel",
    },
];

const NOTIFICATION_CONDITION: &[SettingOption] = &[
    SettingOption {
        label: "When unfocused",
        value: "unfocused",
    },
    SettingOption {
        label: "Always",
        value: "always",
    },
];

const PHASE2_SANDBOX: &[SettingOption] = &[
    SettingOption {
        label: "Workspace write",
        value: "workspace_write",
    },
    SettingOption {
        label: "Danger full access",
        value: "danger_full_access",
    },
];

const APPROVAL: &[SettingOption] = &[
    SettingOption {
        label: "Untrusted",
        value: "untrusted",
    },
    SettingOption {
        label: "On failure",
        value: "on-failure",
    },
    SettingOption {
        label: "On request",
        value: "on-request",
    },
    SettingOption {
        label: "Never",
        value: "never",
    },
];

const SANDBOX: &[SettingOption] = &[
    SettingOption {
        label: "Read only",
        value: "read-only",
    },
    SettingOption {
        label: "Workspace write",
        value: "workspace-write",
    },
    SettingOption {
        label: "Danger full access",
        value: "danger-full-access",
    },
];

macro_rules! setting {
    ($id:literal, $key:literal, $label:literal, $description:literal, $category:ident, $kind:expr, $default:literal, $effect:literal) => {
        SettingDefinition {
            id: $id,
            key: $key,
            label: $label,
            description: $description,
            category: Category::$category,
            kind: $kind,
            default: $default,
            takes_effect: $effect,
        }
    };
}

const DEFINITIONS: &[SettingDefinition] = &[
    setting!(
        "models-manager",
        "",
        "Manage models & providers",
        "Add providers, discover models, and edit capability declarations",
        Models,
        SettingKind::Subpage(Subpage::Models),
        "",
        "Immediately in the catalog"
    ),
    setting!(
        "default-provider",
        "model_provider",
        "Default provider",
        "Provider used by new sessions; changing it resets the model override to that provider's default",
        Models,
        SettingKind::DefaultProvider,
        "astral",
        "New sessions"
    ),
    setting!(
        "default-model",
        "model",
        "Default model",
        "Model used by new sessions within the selected default provider",
        Models,
        SettingKind::DefaultModel,
        "Provider default",
        "New sessions"
    ),
    setting!(
        "default-effort",
        "model_reasoning_effort",
        "Default reasoning effort",
        "Reasoning effort used for new sessions when supported",
        Models,
        SettingKind::Enum(EFFORT),
        "Model default",
        "New sessions"
    ),
    setting!(
        "tool-surface",
        "tools.surface",
        "Tool surface",
        "Expose Claude-style or Codex-style coding tools to the model",
        Tools,
        SettingKind::Enum(TOOL_SURFACE),
        "Claude",
        "Next model request"
    ),
    setting!(
        "web-search-mode",
        "web_search",
        "Web search mode",
        "Disable search, prefer cached results, or allow live search",
        Tools,
        SettingKind::Enum(WEB_SEARCH),
        "Disabled",
        "Next model request"
    ),
    setting!(
        "search-provider",
        "",
        "Search provider",
        "Configure provider, API key, result size, domains, and location",
        Tools,
        SettingKind::Subpage(Subpage::Search),
        "",
        "Next web search"
    ),
    setting!(
        "session-memory",
        "experimental_session_memory_compact",
        "Session Memory Compact",
        "Continuously maintain a compact session summary plus an original tail",
        Memory,
        SettingKind::Bool,
        "Off",
        "Next extraction or compact"
    ),
    setting!(
        "session-memory-init",
        "session_memory_minimum_message_tokens_to_init",
        "Initialize after tokens",
        "Minimum context tokens before the first session-memory extraction",
        Memory,
        SettingKind::Integer,
        "100000",
        "Next extraction"
    ),
    setting!(
        "session-memory-growth",
        "session_memory_minimum_tokens_between_update",
        "Update after token growth",
        "Minimum token growth between session-memory updates",
        Memory,
        SettingKind::Integer,
        "20000",
        "Next extraction"
    ),
    setting!(
        "session-memory-tools",
        "session_memory_tool_calls_between_updates",
        "Update after tool calls",
        "Minimum tool calls between session-memory updates",
        Memory,
        SettingKind::Integer,
        "10",
        "Next extraction"
    ),
    setting!(
        "compact-memory",
        "memories.compact_memory",
        "Long-term memory on compact",
        "Extract memories before compact: off, background enqueue, or blocking",
        Memory,
        SettingKind::Enum(COMPACT_MEMORY),
        "Off",
        "Next /compact"
    ),
    setting!(
        "generate-memories",
        "memories.generate_memories",
        "Generate memories",
        "Allow new threads to produce durable long-term memories",
        Memory,
        SettingKind::Bool,
        "On",
        "New sessions"
    ),
    setting!(
        "use-memories",
        "memories.use_memories",
        "Use memories",
        "Inject relevant durable memories into model context",
        Memory,
        SettingKind::Bool,
        "On",
        "Next model request"
    ),
    setting!(
        "memory-tools",
        "memories.dedicated_tools",
        "Dedicated memory tools",
        "Expose dedicated memory tools to the model",
        Memory,
        SettingKind::Bool,
        "Off",
        "Next model request"
    ),
    setting!(
        "memory-external",
        "memories.disable_on_external_context",
        "Disable with external context",
        "Avoid generating memories after MCP or web context pollutes a thread",
        Memory,
        SettingKind::Bool,
        "Off",
        "New sessions"
    ),
    setting!(
        "theme",
        "tui.theme",
        "Theme",
        "Preview and persist the Astral terminal theme",
        Appearance,
        SettingKind::Theme,
        "Automatic",
        "Preview immediately"
    ),
    setting!(
        "animations",
        "tui.animations",
        "Animations",
        "Enable welcome animation, shimmer, and activity effects",
        Appearance,
        SettingKind::Bool,
        "On",
        "Immediately"
    ),
    setting!(
        "tooltips",
        "tui.show_tooltips",
        "Tooltips",
        "Show startup tips and discoverability hints",
        Appearance,
        SettingKind::Bool,
        "On",
        "Next startup"
    ),
    setting!(
        "vim-mode",
        "tui.vim_mode_default",
        "Vim mode by default",
        "Start the composer in Vim normal mode",
        Appearance,
        SettingKind::Bool,
        "Off",
        "Next startup"
    ),
    setting!(
        "raw-output",
        "tui.raw_output_mode",
        "Raw output mode",
        "Start with copy-friendly raw transcript output",
        Appearance,
        SettingKind::Bool,
        "Off",
        "Next startup"
    ),
    setting!(
        "alternate-screen",
        "tui.alternate_screen",
        "Alternate screen",
        "Choose fullscreen terminal buffering or inline scrollback",
        Appearance,
        SettingKind::Enum(ALT_SCREEN),
        "Auto",
        "Next startup"
    ),
    setting!(
        "notifications",
        "tui.notifications",
        "Notifications",
        "Enable terminal notifications when Astral needs attention",
        Appearance,
        SettingKind::Bool,
        "On",
        "Immediately"
    ),
    setting!(
        "notification-method",
        "tui.notification_method",
        "Notification method",
        "Choose automatic terminal integration, OSC 9, or the terminal bell",
        Appearance,
        SettingKind::Enum(NOTIFICATION_METHOD),
        "Automatic",
        "Next notification"
    ),
    setting!(
        "notification-condition",
        "tui.notification_condition",
        "Notification condition",
        "Notify only while the terminal is unfocused or always",
        Appearance,
        SettingKind::Enum(NOTIFICATION_CONDITION),
        "When unfocused",
        "Next notification"
    ),
    setting!(
        "session-picker",
        "tui.session_picker_view",
        "Session picker layout",
        "Choose a comfortable or dense resume and fork list",
        Appearance,
        SettingKind::Enum(SESSION_PICKER),
        "Dense",
        "Next picker"
    ),
    setting!(
        "default-permissions",
        "default_permissions",
        "Default permission profile",
        "Permission profile applied to new sessions",
        Permissions,
        SettingKind::PermissionProfile,
        "Workspace",
        "New sessions"
    ),
    setting!(
        "cached-fold",
        "experimental_anthropic_cached_fold",
        "Anthropic cached folding",
        "Fold older Anthropic tool results while preserving prompt-cache structure",
        Advanced,
        SettingKind::Bool,
        "Off",
        "Next model request"
    ),
    setting!(
        "approval-policy",
        "approval_policy",
        "Legacy approval policy",
        "Legacy approval behavior; named permission profiles are preferred",
        Advanced,
        SettingKind::Enum(APPROVAL),
        "On request",
        "New sessions"
    ),
    setting!(
        "sandbox-mode",
        "sandbox_mode",
        "Legacy sandbox mode",
        "Legacy sandbox behavior; named permission profiles are preferred",
        Advanced,
        SettingKind::Enum(SANDBOX),
        "Workspace write",
        "New sessions"
    ),
    setting!(
        "memory-extract-model",
        "memories.extract_model",
        "Memory extraction model",
        "Optional model override used to summarize threads",
        Advanced,
        SettingKind::Text,
        "Default model",
        "Next extraction"
    ),
    setting!(
        "memory-consolidation-model",
        "memories.consolidation_model",
        "Memory consolidation model",
        "Optional model override used for global memory consolidation",
        Advanced,
        SettingKind::Text,
        "Default model",
        "Next consolidation"
    ),
    setting!(
        "memory-max-raw",
        "memories.max_raw_memories_for_consolidation",
        "Raw memories retained",
        "Maximum recent raw memories kept for global consolidation",
        Advanced,
        SettingKind::Integer,
        "256",
        "Next consolidation"
    ),
    setting!(
        "memory-unused-days",
        "memories.max_unused_days",
        "Maximum unused days",
        "Exclude memories that have not been used for this many days",
        Advanced,
        SettingKind::Integer,
        "30",
        "Next consolidation"
    ),
    setting!(
        "memory-rollout-age",
        "memories.max_rollout_age_days",
        "Maximum rollout age",
        "Ignore rollout candidates older than this many days",
        Advanced,
        SettingKind::Integer,
        "10",
        "Next extraction"
    ),
    setting!(
        "memory-rollout-count",
        "memories.max_rollouts_per_startup",
        "Rollouts per startup",
        "Maximum rollout candidates processed in one pass",
        Advanced,
        SettingKind::Integer,
        "2",
        "Next startup"
    ),
    setting!(
        "memory-rollout-idle",
        "memories.min_rollout_idle_hours",
        "Minimum rollout idle time",
        "Wait this many hours after thread activity before extracting memories",
        Advanced,
        SettingKind::Integer,
        "6",
        "Next extraction"
    ),
    setting!(
        "memory-rate-limit",
        "memories.min_rate_limit_remaining_percent",
        "Minimum rate-limit remaining",
        "Only start memory work above this remaining percentage",
        Advanced,
        SettingKind::Integer,
        "25",
        "Next startup"
    ),
    setting!(
        "memory-phase2-sandbox",
        "memories.phase2_sandbox",
        "Memory phase 2 sandbox",
        "Sandbox policy for global memory consolidation",
        Advanced,
        SettingKind::Enum(PHASE2_SANDBOX),
        "Workspace write",
        "Next consolidation"
    ),
    setting!(
        "session-memory-templates",
        "",
        "Session memory templates",
        "Choose built-in, inline, or file-backed summary and update prompts",
        Advanced,
        SettingKind::Subpage(Subpage::SessionMemoryTemplates),
        "",
        "Next extraction"
    ),
    setting!(
        "unstable-warning",
        "suppress_unstable_features_warning",
        "Suppress unstable feature warning",
        "Hide startup warnings for enabled under-development features",
        Advanced,
        SettingKind::Bool,
        "Off",
        "Next startup"
    ),
];

pub(crate) const fn definitions() -> &'static [SettingDefinition] {
    DEFINITIONS
}
