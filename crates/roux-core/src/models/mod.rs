mod events;
mod keymap;
mod layout;
mod notification;
mod profile;
mod project;
mod session;
mod settings;
mod task;
mod user_terminal_themes;
mod watch;
mod worktree;

pub use events::{RouxCommand, SessionExitPayload, SessionExitReason};
pub use keymap::{
    merge_keymaps, parse_keymap_kdl, Bind, HudMode, KeyRef, KeymapAction, KeymapParseError,
    KeymapTree, KeymapWarning, Modifier, ParsedKeymap, Prefix,
};
pub use layout::{
    parse_layout_kdl, LayoutPaneNode, LayoutParseError, LayoutProfileRef, LayoutSource, LayoutSpec,
    LayoutSplitDirection,
};
pub use notification::{
    ActionKind, Notification, NotificationAction, NotificationEvent, NotificationLevel,
    NotificationRequest, NotificationSource,
};
pub use profile::{ProfileSource, Provider, SpawnProfile, StartupBehavior};
pub use project::Project;
pub use session::{Session, SessionStatus};
pub use settings::{
    CursorStyle, GroupBy, LibrarySource, LibrarySourceKind, RouxSettings, SkillSyncMode,
    StatusBarPosition, TabPosition, UpdateChannel, WorktreeCleanupMode, WorktreeDefaultBase,
    WorktreeProvider,
};
pub use task::{KeepOpen, TaskDefinition, TaskGroup};
pub use user_terminal_themes::{
    scan_user_terminal_themes, TerminalAnsiPalette, TerminalThemePalette,
    UserTerminalTheme, UserThemeError,
};
pub use watch::*;
pub use worktree::{Worktree, WorktrunkMetadata};
