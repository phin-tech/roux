use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod attach;
mod cli_socket;
mod daemon;
mod daemon_log;
mod mcp;
mod paths;
mod platform;

use cli_socket::send_socket_command;

#[derive(Parser)]
#[command(name = "roux", about = "Roux terminal manager CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Hook commands. Status variants are called by hooks in ~/.claude/settings.json.
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Install Roux integrations into external agent tools
    Install {
        #[command(subcommand)]
        action: InstallAction,
    },
    /// Show current session statuses
    Status,
    /// Clear all session status files
    Clear,

    // ── Socket commands ──────────────────────────────────────
    /// Open (or focus) a Roux session for a directory, then bring the app to front
    App {
        /// Directory path. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: String,
    },
    /// Split the current pane
    Split {
        /// Direction: horizontal or vertical
        #[arg(short, long, default_value = "horizontal")]
        direction: String,
    },
    /// Session management and introspection
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Open a shell pane
    Shell {
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Focus a pane or session
    Focus {
        /// Pane ID to focus
        #[arg(short, long)]
        pane: Option<String>,
        /// Session ID to focus
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Run a command in a new pane
    Run {
        /// The command to run
        command: String,
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Attach this terminal to a daemon-owned PTY
    Attach {
        /// Daemon PTY id. When omitted, --session or $ROUX_SESSION_ID is used.
        #[arg(value_name = "PTY_ID", conflicts_with = "session")]
        target: Option<String>,
        /// Session id whose primary daemon PTY should be attached
        #[arg(short, long)]
        session: Option<String>,
        /// Maximum retained output bytes to replay on attach
        #[arg(long, default_value_t = 65536)]
        max_bytes: usize,
        /// Do not forward stdin into the daemon PTY
        #[arg(long)]
        no_input: bool,
    },
    /// Multi-scoped notes vault (experimental).
    ///
    /// Read, append, write, and search notes across four scopes
    /// (global / project / repo / session). Session and scope context is
    /// resolved from `$ROUX_SESSION_ID`. See `docs/features/notes.md`.
    Notes {
        #[command(subcommand)]
        action: NotesAction,
    },
    /// Attach and read session/work item documents
    #[command(visible_alias = "doc")]
    Document {
        #[command(subcommand)]
        action: DocumentAction,
    },
    /// Push a notification into Roux's notification service
    Notify {
        /// Notification title (required unless --json is used)
        #[arg(short = 't', long)]
        title: Option<String>,
        /// Notification body
        #[arg(short = 'b', long)]
        body: Option<String>,
        /// Optional subtitle (renders between title and body)
        #[arg(long)]
        subtitle: Option<String>,
        /// Severity: info | success | attention | warning | error
        #[arg(short = 'l', long, default_value = "info")]
        level: String,
        /// Explicit session id; falls back to --cwd, then $ROUX_SESSION_ID, then global
        #[arg(short = 's', long)]
        session: Option<String>,
        /// Working directory used to resolve a session if --session is not given
        #[arg(long)]
        cwd: Option<String>,
        /// Free-form source tag (used as the NotificationSource::Cli provider hint in logs)
        #[arg(long)]
        source: Option<String>,
        /// Read a JSON payload from stdin instead of individual flags.
        /// The JSON should match NotificationRequest: {level, title, body?, subtitle?, sessionId?, source?}
        #[arg(long)]
        json: bool,
    },
    /// Start the Roux MCP stdio server
    Mcp,
    /// Start or inspect the headless Roux runtime daemon
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },
    /// Manage Kanban board work items
    #[command(name = "work-item", visible_alias = "kanban")]
    WorkItem {
        #[command(subcommand)]
        action: WorkItemAction,
    },
    /// Manage agent aliases — stable, restart-durable identity for sessions
    Alias {
        #[command(subcommand)]
        action: AliasAction,
    },
    /// Direct, ack-able mail addressed to an alias
    Mailbox {
        #[command(subcommand)]
        action: MailboxAction,
    },
    /// Topic-based broadcast over the same event store
    Bus {
        #[command(subcommand)]
        action: BusAction,
    },
}

#[derive(Subcommand)]
enum MailboxAction {
    /// Post a message to a recipient alias and/or topic
    Post {
        /// Body text
        body: String,
        /// Recipient alias (at least one of --to or --topic required)
        #[arg(short = 't', long)]
        to: Option<String>,
        /// Topic name for broadcast (mailbox + bus addressing combine)
        #[arg(long)]
        topic: Option<String>,
        /// Optional subject line
        #[arg(long)]
        subject: Option<String>,
        /// Kind: task | result | question | fyi | signal (default: task)
        #[arg(long)]
        kind: Option<String>,
        /// Project scope
        #[arg(short = 'p', long)]
        project: Option<String>,
        /// Thread key — copy from a previous event id to thread replies
        #[arg(long)]
        correlation_id: Option<String>,
        /// Override sender (default: calling session's primary alias)
        #[arg(long)]
        from: Option<String>,
    },
    /// Peek at unread mail (does not change read state)
    Peek {
        /// Recipient alias (default: calling session's primary alias)
        #[arg(short = 'a', long)]
        alias: Option<String>,
        /// Only show unread events
        #[arg(short = 'u', long)]
        unread: bool,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(short = 'l', long)]
        limit: Option<u32>,
    },
    /// Drain unread mail and mark it read
    Read {
        #[arg(short = 'a', long)]
        alias: Option<String>,
        /// Also ack each drained event
        #[arg(long)]
        ack: bool,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(short = 'l', long)]
        limit: Option<u32>,
    },
    /// Ack a specific event (terminal "I've handled this" state)
    Ack {
        event_id: String,
        /// Optional short result string visible to the sender
        #[arg(short = 'r', long)]
        result: Option<String>,
        #[arg(short = 'a', long)]
        alias: Option<String>,
    },
    /// Count unread mail
    Count {
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
    },
    /// Clear read mail (read events drop from your view; unread persist)
    Clear {
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
    },
    /// Reply to an event, preserving its correlation_id for threading
    Reply {
        event_id: String,
        body: String,
        #[arg(long)]
        subject: Option<String>,
        /// Kind (default: result)
        #[arg(long)]
        kind: Option<String>,
    },
    /// List events I've sent with their per-recipient state
    Sent {
        /// Filter to a single recipient alias
        #[arg(long)]
        to: Option<String>,
        /// Override sender lookup (default: calling session's primary alias)
        #[arg(long)]
        sender: Option<String>,
        #[arg(short = 'l', long)]
        limit: Option<u32>,
    },
    /// Stream new mail as it arrives (push, no polling). Prints each
    /// event as a JSON line; exits on Ctrl+C or socket close.
    Watch {
        /// Recipient alias (default: calling session's primary alias)
        #[arg(short = 'a', long)]
        alias: Option<String>,
        /// Also ack each delivered event with a "watched" marker
        #[arg(long)]
        ack: bool,
        /// Skip the initial unread backlog and only stream future events
        #[arg(long)]
        no_backlog: bool,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
    },
    /// Unsend an event you posted. Only works if no recipient has
    /// acked yet — once anyone confirmed delivery the audit trail is
    /// preserved.
    Unsend {
        event_id: String,
        /// Sender alias to retract on behalf of (default: calling
        /// pane's bound alias).
        #[arg(short = 'a', long)]
        alias: Option<String>,
    },
    /// Dismiss a single event from your inbox view (read or unread).
    /// The event itself is preserved; only your view loses it.
    Dismiss {
        event_id: String,
        #[arg(short = 'a', long)]
        alias: Option<String>,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon in the background
    Start,
    /// Ask the daemon to stop gracefully
    Stop,
    /// Stop the daemon if running, then start it in the background
    Restart,
    /// Clear stale daemon coordination files when the daemon is unreachable
    Clear,
    /// Query the daemon-only status endpoint
    Status,
    /// Show daemon runtime logs
    Logs {
        /// Number of lines to print before exiting
        #[arg(short = 'n', long, default_value_t = 200)]
        lines: usize,
        /// Follow the log file with `tail -f`
        #[arg(short, long)]
        follow: bool,
    },
    /// Persist a daemon socket endpoint for future CLI commands
    Connect {
        /// Socket endpoint, e.g. tcp://100.73.57.24:7777
        socket: String,
        /// Auth token for TCP daemon endpoints
        #[arg(long)]
        auth_token: Option<String>,
    },
    /// Clear a persisted daemon socket endpoint
    Disconnect,
    /// Start a daemon-owned shell command and return its process id
    Run {
        /// Shell command to run inside the daemon
        command: String,
        /// Working directory for the command
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Poll retained output for a daemon-owned process
    Output {
        /// Daemon process id returned by `roux daemon run`
        id: String,
        /// Maximum retained output bytes to return
        #[arg(long, default_value_t = 65536)]
        max_bytes: usize,
    },
    /// List daemon-owned processes
    Processes,
    /// Kill a daemon-owned process
    Kill {
        /// Daemon process id
        id: String,
    },
    /// Start a daemon-owned PTY task and return its PTY id
    PtyRun {
        /// Shell command to run inside the PTY
        command: String,
        /// Working directory for the PTY
        #[arg(short, long)]
        working_dir: Option<String>,
        /// Session id metadata for the PTY
        #[arg(long)]
        session_id: Option<String>,
        /// Pane id metadata for the PTY
        #[arg(long)]
        pane_id: Option<String>,
        /// Profile metadata for the PTY
        #[arg(long)]
        profile: Option<String>,
        /// Initial PTY columns
        #[arg(long)]
        cols: Option<u16>,
        /// Initial PTY rows
        #[arg(long)]
        rows: Option<u16>,
    },
    /// Poll retained output for a daemon-owned PTY
    PtyOutput {
        /// Daemon PTY id returned by `roux daemon pty-run`
        id: String,
        /// Maximum retained output bytes to return
        #[arg(long, default_value_t = 65536)]
        max_bytes: usize,
    },
    /// List daemon-owned PTYs
    Ptys,
    /// Write text into a daemon-owned PTY
    PtyWrite {
        /// Daemon PTY id
        id: String,
        /// Text to write
        data: String,
    },
    /// Resize a daemon-owned PTY
    PtyResize {
        /// Daemon PTY id
        id: String,
        /// PTY columns
        cols: u16,
        /// PTY rows
        rows: u16,
    },
    /// Kill a daemon-owned PTY
    PtyKill {
        /// Daemon PTY id
        id: String,
    },
    /// List daemon-owned watches
    Watches,
}

#[derive(Subcommand)]
enum WorkItemAction {
    /// List work items
    List {
        /// Filter to one project id
        #[arg(short, long)]
        project: Option<String>,
        /// Include archived cards
        #[arg(long)]
        include_archived: bool,
    },
    /// Create a work item card
    Create(WorkItemCreateArgs),
    /// Update a work item card
    Update(WorkItemUpdateArgs),
    /// Move a work item to a new status
    Move {
        /// Work item id
        id: String,
        /// Status: todo | ready | doing | review | done
        status: String,
        /// Sort order within the destination column
        #[arg(long)]
        sort_order: Option<f64>,
    },
    /// Delete a work item card
    Delete {
        /// Work item id
        id: String,
    },
    /// Archive a work item card
    Archive {
        /// Work item id
        id: String,
    },
    /// Restore an archived work item card
    Restore {
        /// Work item id
        id: String,
    },
    /// Plan a work item with a daemon-owned planning run
    Plan(WorkItemPlanArgs),
    /// Start a work item as a daemon-owned run
    #[command(alias = "dispatch")]
    Start(WorkItemStartArgs),
    /// Manage work item review handoff
    Review {
        #[command(subcommand)]
        action: WorkItemReviewAction,
    },
    /// Accept the current review stage
    Accept {
        /// Work item id
        id: String,
    },
    /// List work item runs
    Runs {
        /// Filter to one work item id
        #[arg(short = 'w', long = "work-item")]
        work_item: Option<String>,
    },
    /// List events for a work item run
    Events {
        /// Work item run id
        run_id: String,
    },
    /// Stop a daemon-owned work item run
    Stop {
        /// Work item run id
        run_id: String,
    },
    /// Manage card-level decision prompts
    Decision {
        #[command(subcommand)]
        action: WorkItemDecisionAction,
    },
    /// Bulk import work items from a JSON file or inline JSON array
    Import {
        /// File containing { "items": [...] }
        #[arg(long)]
        path: Option<String>,
        /// Inline JSON array of items
        #[arg(long = "items-json")]
        items_json: Option<String>,
    },
    /// Stream work item board events as JSON lines
    Watch,
}

#[derive(Subcommand)]
enum WorkItemReviewAction {
    /// Request review for an implementation run and enter the current review stage
    Request {
        /// Work item run id
        run_id: String,
        /// Short implementation summary to show in the review package
        #[arg(long)]
        summary: Option<String>,
        /// Test/check command that was run. Repeat for multiple entries.
        #[arg(long = "test")]
        tests: Vec<String>,
        /// Changed file path to show in the review package. Repeat for multiple entries.
        #[arg(long = "changed-file")]
        changed_files: Vec<String>,
    },
    /// Request changes for a reviewed run and preserve the current review stage
    RequestChanges {
        /// Work item run id or work item id
        target: String,
        /// Human feedback to attach to the work item
        #[arg(long)]
        note: String,
        /// Destination status for the card: doing | ready
        #[arg(long)]
        status: Option<String>,
    },
    /// Accept the current review stage; final review moves the card to Done
    Accept {
        /// Work item id
        id: String,
    },
}

struct WorkItemReviewRequestArgs {
    run_id: String,
    summary: Option<String>,
    tests: Vec<String>,
    changed_files: Vec<String>,
}

#[derive(Subcommand)]
enum DocumentAction {
    /// Attach text or a UTF-8 file snapshot to a session or work item
    Attach(DocumentAttachArgs),
    /// List attached documents
    List(DocumentListArgs),
    /// Read an attached document by id
    Get {
        /// Attachment id or fully qualified document id
        id: String,
    },
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("document_attach_target")
        .required(true)
        .args(["session", "work_item"])
))]
#[command(group(
    ArgGroup::new("document_attach_source")
        .required(true)
        .args(["text", "file"])
))]
struct DocumentAttachArgs {
    /// Session id to attach this document to
    #[arg(short, long)]
    session: Option<String>,
    /// Work item id to attach this document to
    #[arg(short = 'w', long = "work-item")]
    work_item: Option<String>,
    /// Human-readable title for the document
    #[arg(long)]
    title: Option<String>,
    /// Inline text content to attach
    #[arg(long)]
    text: Option<String>,
    /// UTF-8 file whose current contents should be attached
    #[arg(long)]
    file: Option<String>,
    /// MIME type hint for consumers
    #[arg(long = "mime-type")]
    mime_type: Option<String>,
}

#[derive(Args)]
#[command(group(ArgGroup::new("document_list_target").args(["session", "work_item"])))]
struct DocumentListArgs {
    /// Filter to one session id
    #[arg(short, long)]
    session: Option<String>,
    /// Filter to one work item id
    #[arg(short = 'w', long = "work-item")]
    work_item: Option<String>,
}

#[derive(Args)]
struct WorkItemCreateArgs {
    /// Card title
    title: String,
    /// Card body/description
    #[arg(short, long)]
    body: Option<String>,
    /// Initial status: todo | ready | doing | review | done
    #[arg(short, long)]
    status: Option<String>,
    /// Repo path used when starting the card
    #[arg(long)]
    repo_path: Option<String>,
    /// Autonomous agent profile used when starting the card
    #[arg(long)]
    agent_profile: Option<String>,
    /// Base ref for the card's dedicated worktree
    #[arg(long)]
    base_branch: Option<String>,
    /// Dedicated worktree path to reuse for the card
    #[arg(long)]
    worktree_path: Option<String>,
    /// Branch name for card worktree creation
    #[arg(long)]
    branch: Option<String>,
    /// Fetch before creating/checking out the card worktree
    #[arg(long)]
    fetch_first: bool,
    /// Project id
    #[arg(short, long)]
    project: Option<String>,
    /// Parent work item id
    #[arg(long)]
    parent: Option<String>,
    /// Sort order within the column
    #[arg(long)]
    sort_order: Option<f64>,
}

#[derive(Args)]
struct WorkItemUpdateArgs {
    /// Work item id
    id: String,
    /// New card title
    #[arg(short, long)]
    title: String,
    /// Card body/description
    #[arg(short, long)]
    body: Option<String>,
    /// Status: todo | ready | doing | review | done
    #[arg(short, long)]
    status: Option<String>,
    /// Repo path used when starting the card
    #[arg(long)]
    repo_path: Option<String>,
    /// Autonomous agent profile used when starting the card
    #[arg(long)]
    agent_profile: Option<String>,
    /// Base ref for the card's dedicated worktree
    #[arg(long)]
    base_branch: Option<String>,
    /// Dedicated worktree path to reuse for the card
    #[arg(long)]
    worktree_path: Option<String>,
    /// Branch name for card worktree creation
    #[arg(long, num_args = 0..=1)]
    branch: Option<Option<String>>,
    /// Fetch before creating/checking out the card worktree; pass --fetch-first=false to disable
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "true")]
    fetch_first: Option<bool>,
    /// Project id
    #[arg(short, long)]
    project: Option<String>,
    /// Parent work item id
    #[arg(long)]
    parent: Option<String>,
    /// Sort order within the column
    #[arg(long)]
    sort_order: Option<f64>,
}

#[derive(Args)]
struct WorkItemStartArgs {
    /// Work item id
    id: String,
    /// Spawn profile id, e.g. claude, codex, plain-shell
    #[arg(short = 'P', long)]
    profile: Option<String>,
    /// Repo path override; otherwise the card project's repo is used
    #[arg(long)]
    repo_path: Option<String>,
    /// Session name override
    #[arg(long)]
    name: Option<String>,
    /// Worktree path to use/create
    #[arg(long)]
    worktree_path: Option<String>,
    /// Branch name for session/worktree creation
    #[arg(long)]
    branch: Option<String>,
    /// Base ref for worktree branch creation
    #[arg(long)]
    base: Option<String>,
    /// Fetch before creating/checking out the worktree
    #[arg(long)]
    fetch_first: bool,
    /// Start without an attached plan
    #[arg(long)]
    force_start: bool,
    /// Focus the dispatched implementation run on fixing failing CI for the card's PR
    #[arg(long)]
    fix_ci: bool,
}

#[derive(Args)]
struct WorkItemPlanArgs {
    /// Work item id
    id: String,
    /// Spawn profile id, e.g. claude or codex
    #[arg(short = 'P', long)]
    profile: Option<String>,
    /// Repo path override; otherwise the card/project repo or daemon cwd is used
    #[arg(long)]
    repo_path: Option<String>,
    /// Session name override
    #[arg(long)]
    name: Option<String>,
    /// Existing worktree path for the planning session
    #[arg(long)]
    worktree_path: Option<String>,
    /// Stop any active planning run and create a fresh one
    #[arg(long)]
    replace_active: bool,
}

#[derive(Subcommand)]
enum WorkItemDecisionAction {
    /// Create a pending decision on a run
    Create(WorkItemDecisionCreateArgs),
    /// List pending decisions
    List {
        /// Filter to one work item id
        #[arg(short = 'w', long = "work-item")]
        work_item: Option<String>,
    },
    /// Resolve a pending decision
    Resolve {
        /// Decision id
        id: String,
        /// Selected option value
        value: String,
        /// Actor label for audit history
        #[arg(long)]
        resolved_by: Option<String>,
    },
}

#[derive(Args)]
struct WorkItemDecisionCreateArgs {
    /// Work item run id
    run_id: String,
    /// Prompt shown to the human
    question: String,
    /// Option as value=label. If =label is omitted, label defaults to value.
    #[arg(short = 'o', long = "option", required = true)]
    options: Vec<String>,
    /// Default option value used by timeout
    #[arg(long)]
    default_value: Option<String>,
    /// Absolute Unix timestamp in seconds
    #[arg(long)]
    timeout_at: Option<u64>,
    /// Relative timeout in seconds
    #[arg(long)]
    timeout_seconds: Option<u64>,
    /// Relative timeout in milliseconds
    #[arg(long)]
    timeout_ms: Option<u64>,
}

#[derive(Subcommand)]
enum BusAction {
    /// Publish an event to a topic (no specific recipient)
    Publish {
        topic: String,
        /// Body text (or empty when only `--structured` is meaningful)
        body: String,
        /// Kind (default: signal)
        #[arg(long)]
        kind: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long)]
        subject: Option<String>,
    },
    /// Tail events. With --topic filters by topic; otherwise firehose
    Tail {
        #[arg(short = 't', long)]
        topic: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
        #[arg(short = 'l', long)]
        limit: Option<u32>,
    },
    /// Subscribe an alias to topic events matching a glob pattern.
    /// `*` matches one segment, `**` matches many (e.g. `repo-a.*`,
    /// `**.completed`). Defaults `--alias` to the current pane's alias.
    Subscribe {
        /// Glob pattern (validated server-side). Quote in shells that
        /// expand `*` themselves.
        pattern: String,
        /// Alias receiving deliveries. Defaults to the calling pane's
        /// auto-claimed alias.
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
    },
    /// Remove a subscription by id.
    Unsubscribe { id: String },
    /// List subscriptions. Without --alias / --project, lists all.
    Subscriptions {
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "project")]
        global: bool,
    },
}

#[derive(Subcommand)]
enum AliasAction {
    /// Bind an alias to a session (defaults to the current session)
    Set {
        /// Alias name (lowercase, hyphens allowed; reserved names rejected)
        alias: String,
        /// Target session id; defaults to $ROUX_SESSION_ID
        #[arg(short, long)]
        session: Option<String>,
        /// Project scope; aliases with the same name in different projects are independent
        #[arg(short, long)]
        project: Option<String>,
        /// Override an existing binding to a different session
        #[arg(short, long)]
        force: bool,
    },
    /// Release an alias's binding (queued mail persists for the next claim)
    Unset {
        alias: String,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// Current session claims the alias
    Claim {
        alias: String,
        #[arg(short, long)]
        project: Option<String>,
        /// Override existing binding
        #[arg(long)]
        steal: bool,
    },
    /// List aliases
    List {
        /// Filter to a specific project scope
        #[arg(short, long)]
        project: Option<String>,
        /// Only list global (project-less) aliases
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Only list aliases that are currently unbound
        #[arg(long)]
        only_unbound: bool,
    },
    /// Resolve an alias to its current binding
    Get {
        alias: String,
        #[arg(short, long)]
        project: Option<String>,
    },
    /// List aliases bound to a session (defaults to the current session)
    Whoami {
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Add a pane to a group alias. Creates the alias if it doesn't
    /// exist. The pane is `--pane` if given, else `$ROUX_PANE_ID`.
    AddMember {
        alias: String,
        #[arg(long)]
        pane: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
    },
    /// Remove a pane from a group alias's membership.
    RemoveMember {
        alias: String,
        #[arg(long)]
        pane: Option<String>,
        #[arg(short = 'p', long)]
        project: Option<String>,
    },
    /// Set the consumption mode for a group alias. `competing` (the
    /// default) is a work-queue: the first member to ack claims the
    /// event. `broadcast` is reserved for the per-member ReadState
    /// follow-up; today it falls back to competing semantics.
    Mode {
        alias: String,
        /// `competing` | `broadcast`
        mode: String,
        #[arg(short = 'p', long)]
        project: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Create a new session
    Create {
        /// Session name
        #[arg(short, long)]
        name: Option<String>,
        /// Working directory (existing worktree path or repo path). Default: cwd
        #[arg(short, long)]
        working_dir: Option<String>,
        /// Create a new worktree from this branch. Uses --working-dir (or cwd) as the repo to branch from.
        #[arg(long)]
        worktree_branch: Option<String>,
        /// Git ref to branch the new worktree from (e.g. "main", "origin/main", "abc123").
        /// Refs starting with "origin/" trigger a `git fetch origin` first.
        /// Ignored when --worktree-branch is not set.
        #[arg(long)]
        from: Option<String>,
        /// Spawn profile id (e.g. "claude", "plain-shell", "codex", user profile id). Default: claude
        #[arg(short = 'P', long)]
        profile: Option<String>,
        /// Extra flag passed to the agent binary (repeatable; values may begin with --)
        #[arg(short = 'f', long = "flag", allow_hyphen_values = true)]
        flags: Vec<String>,
        /// Text to send to the session's primary PTY immediately after it starts
        /// (with a trailing Enter). Handy for kicking off an agent task at
        /// creation time without a separate `session send` call.
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Send text to a session's PTY (appends \r by default; use --no-enter for raw)
    Send {
        /// The text to send
        text: String,
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
        /// Pane id (falls back to $ROUX_PANE_ID). Takes priority over --pane-type.
        #[arg(short, long)]
        pane: Option<String>,
        /// Target the first pane of this type in the session (e.g. shell, claude, command).
        /// Ignored when --pane is given.
        #[arg(long)]
        pane_type: Option<String>,
        /// Send raw text without appending \r (Enter). By default Enter is sent.
        #[arg(long = "no-enter")]
        no_enter: bool,
    },
    /// Get the current state of a session (JSON)
    Poll {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// List all sessions (JSON)
    List,
    /// Rename a session (sets a name override that takes precedence over
    /// the branch-derived label in the UI).
    Rename {
        /// New name. Pass an empty string to clear the override.
        #[arg(short, long)]
        name: String,
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Archive (soft-kill) a session — kills its PTYs and moves it to history.
    Kill {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Pane commands for a session
    Panes {
        #[command(subcommand)]
        action: PaneAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Claude status: working
    Working,
    /// Claude status: idle
    Idle,
    /// Claude status: attention
    Attention,
    /// Claude status: error
    Error,
    /// Claude status: disconnected
    Disconnected,
    /// Show configured Roux automation hooks
    Show(HookShowArgs),
    /// Run a Roux automation hook through the running app
    Run(Box<HookRunArgs>),
}

#[derive(Subcommand)]
enum InstallAction {
    /// Install Roux hooks into an agent config
    Hooks(InstallHooksArgs),
}

#[derive(Args)]
struct InstallHooksArgs {
    /// Agent whose hooks should be installed
    #[arg(long, value_enum)]
    agent: InstallHooksAgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InstallHooksAgent {
    Claude,
}

#[derive(Args)]
struct HookShowArgs {
    /// Repo path whose project hooks should be included
    #[arg(long)]
    repo_path: Option<String>,
}

#[derive(Args)]
struct HookRunArgs {
    /// Hook event name, e.g. post-watch-success
    event: String,
    #[arg(long)]
    repo_path: Option<String>,
    #[arg(long)]
    worktree_path: Option<String>,
    #[arg(long)]
    branch: Option<String>,
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    task: Option<String>,
    #[arg(long)]
    scope: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    /// Extra args passed into the hook context. Use `--` before values.
    #[arg(last = true)]
    extra: Vec<String>,
}

#[derive(Subcommand)]
enum PaneAction {
    /// List panes for a session (JSON)
    List {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Create a new pane in a session
    Create {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
        /// Spawn profile id (e.g. "plain-shell", "claude", "codex", user profile id). Default: plain-shell
        #[arg(short = 'P', long)]
        profile: Option<String>,
        /// Split direction from the active pane (horizontal|vertical). Default: horizontal
        #[arg(short, long, default_value = "horizontal")]
        direction: String,
        /// Working directory for the new pane. Default: session's worktree_path
        #[arg(short, long)]
        working_dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum NotesAction {
    /// Global scope — your personal catch-all, shared across every session.
    Global {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Project scope — shared by every session tagged with the same project.
    Project {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Repo scope — shared by every session working in the same repo.
    Repo {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Session scope — the current session's personal scratchpad / log.
    Session {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Search across the vault by frontmatter tags + inline `#tag` occurrences.
    Search {
        /// Required tag filter. Pass multiple times for AND. Hierarchical prefix
        /// matching is on by default (`--tag api` matches `api/tls`).
        #[arg(long = "tag", action = clap::ArgAction::Append, required = true)]
        tags: Vec<String>,
        /// Restrict to one scope's subtree. Default: whole vault.
        #[arg(long)]
        scope: Option<String>,
        /// Disable hierarchical prefix matching (literal tag names only).
        #[arg(long)]
        tag_exact: bool,
    },
    /// Print the vault root path.
    Root,
}

#[derive(Subcommand)]
enum NotesScopeVerb {
    /// Print the note contents to stdout.
    Show {
        /// Target a topic file within the scope dir instead of `notes.md`.
        #[arg(long)]
        topic: Option<String>,
    },
    /// Append to the note. Reads from stdin if `--content` is not supplied.
    Append {
        #[arg(long)]
        topic: Option<String>,
        /// Content to append. If omitted, stdin is used.
        #[arg(long)]
        content: Option<String>,
        /// Prepend a timestamped heading + block-ref for this entry.
        #[arg(long)]
        timestamp: bool,
        /// Union-merge a tag into the file's frontmatter `tags:` list. Repeatable.
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// Replace the note body. Reads from stdin if `--content` is not supplied.
    Write {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// Print the note's absolute filesystem path.
    Path {
        #[arg(long)]
        topic: Option<String>,
        /// Print the scope directory instead of the file.
        #[arg(long)]
        dir: bool,
    },
}

fn status_dir() -> PathBuf {
    platform::status_dir()
}

fn claude_settings_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Could not determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

fn current_roux_cli_path() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|e| format!("Could not determine current roux executable: {e}"))
        .map(|path| path.canonicalize().unwrap_or(path))
}

fn hook_command(cli_path: &Path, status: &str) -> String {
    platform::command_string(cli_path, &["hook", status])
}

fn roux_claude_hooks_config(cli_path: &Path) -> Value {
    json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "working") }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "idle") }
                    ]
                }
            ],
            "PermissionRequest": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "attention") }
                    ]
                }
            ],
            "PreToolUse": [
                {
                    "matcher": "AskUserQuestion",
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "attention") }
                    ]
                }
            ],
            "PostToolUse": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "working") }
                    ]
                }
            ],
            "StopFailure": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "error") }
                    ]
                }
            ],
            "SessionEnd": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "idle") }
                    ]
                }
            ],
            "Notification": [
                {
                    "matcher": "permission_prompt|elicitation_dialog|elicitation_response",
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "attention") }
                    ]
                },
                {
                    "matcher": "elicitation_complete|idle_prompt",
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "idle") }
                    ]
                }
            ],
            "Elicitation": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "attention") }
                    ]
                }
            ],
            "ElicitationResult": [
                {
                    "hooks": [
                        { "type": "command", "command": hook_command(cli_path, "idle") }
                    ]
                }
            ]
        }
    })
}

fn split_command_program(command: &str) -> Option<(&str, &str)> {
    let trimmed = command.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('"') {
        let quote = rest.find('"')?;
        let (program, remainder) = rest.split_at(quote);
        return Some((program, remainder[1..].trim_start()));
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let program = parts.next()?;
    Some((program, parts.next().unwrap_or("").trim_start()))
}

fn command_program_is_roux_cli(program: &str) -> bool {
    let file_name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    matches!(
        file_name.to_ascii_lowercase().as_str(),
        "roux" | "roux.exe" | "roux-cli" | "roux-cli.exe"
    )
}

fn args_start_with_roux_hook_status(args: &str) -> bool {
    let mut args = args.split_whitespace();
    let Some(hook) = args.next() else {
        return false;
    };
    if hook != "hook" {
        return false;
    }

    matches!(args.next(), Some("working" | "idle" | "attention" | "error" | "disconnected"))
}

fn is_roux_hook_command(command: &str) -> bool {
    let Some((program, args)) = split_command_program(command) else {
        return false;
    };
    command_program_is_roux_cli(program) && args_start_with_roux_hook_status(args)
}

fn is_roux_hook(hook_obj: &Value) -> bool {
    hook_obj.get("command").and_then(|c| c.as_str()).map(is_roux_hook_command).unwrap_or(false)
}

fn is_roux_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|hooks| hooks.iter().any(is_roux_hook))
        .unwrap_or(false)
}

fn merge_roux_claude_hooks(settings: &mut Value, cli_path: &Path) -> Result<(), String> {
    let roux = roux_claude_hooks_config(cli_path);
    let roux_hooks = roux.get("hooks").and_then(|hooks| hooks.as_object()).unwrap();

    if settings.get("hooks").is_none() || !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }

    for (event_name, roux_entries) in roux_hooks {
        let roux_entries = roux_entries.as_array().unwrap();
        let existing = settings["hooks"]
            .get(event_name)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut filtered: Vec<Value> = existing
            .into_iter()
            .filter(|entry| !is_roux_hook_entry(entry))
            .filter(|entry| {
                !entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|hooks| {
                        hooks.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains("roux/hook-handler.sh"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .collect();
        filtered.extend(roux_entries.iter().cloned());
        settings["hooks"][event_name] = Value::Array(filtered);
    }

    Ok(())
}

fn install_claude_hooks() -> Result<PathBuf, String> {
    fs::create_dir_all(status_dir()).map_err(|e| format!("Failed to create status dir: {e}"))?;
    let cli_path = current_roux_cli_path()?;
    let settings_path = claude_settings_path()?;

    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read {}: {e}", settings_path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {e}", settings_path.display()))?
    } else {
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        json!({})
    };

    merge_roux_claude_hooks(&mut settings, &cli_path)?;
    let output = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize hooks: {e}"))?;
    fs::write(&settings_path, output)
        .map_err(|e| format!("Failed to write {}: {e}", settings_path.display()))?;
    Ok(settings_path)
}

fn handle_install_action(action: InstallAction) {
    let result = match action {
        InstallAction::Hooks(args) => match args.agent {
            InstallHooksAgent::Claude => install_claude_hooks()
                .map(|path| format!("Installed Claude hooks to {}", path.display())),
        },
    };

    match result {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

/// Build the shared `args` map for receive-side mailbox commands
/// (peek/read/count/clear). Keeps the dispatch arms focused on their
/// command-specific extras.
fn build_mailbox_recv_args(
    alias: Option<String>,
    project: Option<String>,
    global: bool,
    unread: Option<bool>,
    limit: Option<u32>,
) -> Value {
    let mut args = serde_json::Map::new();
    if let Some(a) = alias {
        args.insert("alias".into(), Value::String(a));
    }
    if let Some(p) = project {
        args.insert("project_id".into(), Value::String(p));
    }
    if global {
        args.insert("global".into(), Value::Bool(true));
    }
    if let Some(true) = unread {
        args.insert("unread".into(), Value::Bool(true));
    }
    if let Some(n) = limit {
        args.insert("limit".into(), Value::Number(n.into()));
    }
    Value::Object(args)
}

fn resolve_path(path: &str) -> String {
    let p = std::path::PathBuf::from(path);
    let absolute =
        if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) };
    absolute.canonicalize().unwrap_or(absolute).to_string_lossy().to_string()
}

fn get_session_id() -> Option<String> {
    std::env::var("ROUX_SESSION_ID").ok()
}

fn get_pane_id() -> Option<String> {
    std::env::var("ROUX_PANE_ID").ok()
}

fn get_work_item_id() -> Option<String> {
    std::env::var("ROUX_WORK_ITEM_ID").ok()
}

fn get_work_item_run_id() -> Option<String> {
    std::env::var("ROUX_WORK_ITEM_RUN_ID").ok()
}

/// Pick the session_id and pane_id to send over the wire, given explicit CLI
/// flags and the current env. The invariant is that `$ROUX_PANE_ID` is only
/// meaningful inside its own `$ROUX_SESSION_ID`, so when the caller redirects
/// to a different session we must NOT inherit the env pane — that pane belongs
/// to the calling session and would route the write to the wrong PTY (or, more
/// often, fail outright because pane ids and pty ids live in different
/// namespaces in the backend).
fn resolve_target(
    session: Option<String>,
    pane: Option<String>,
    env_session: Option<String>,
    env_pane: Option<String>,
) -> (Option<String>, Option<String>) {
    match (session, pane) {
        (Some(s), Some(p)) => (Some(s), Some(p)),
        (Some(s), None) => {
            // Only inherit the env pane if it belongs to the same session the
            // caller is targeting.
            let pane = if env_session.as_deref() == Some(s.as_str()) { env_pane } else { None };
            (Some(s), pane)
        }
        (None, Some(p)) => (env_session, Some(p)),
        (None, None) => (env_session, env_pane),
    }
}

fn text_from_content(content: &Value) -> Option<String> {
    match content {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    let obj = part.as_object()?;
                    (obj.get("type").and_then(|v| v.as_str()) == Some("text"))
                        .then(|| obj.get("text").and_then(|v| v.as_str()))
                        .flatten()
                })
                .collect::<Vec<_>>()
                .join(" ");
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    }
}

fn truncate_summary(s: String) -> String {
    const MAX_CHARS: usize = 200;
    if s.chars().count() <= MAX_CHARS {
        return s;
    }
    let mut out = s.chars().take(MAX_CHARS - 3).collect::<String>();
    out.push_str("...");
    out
}

fn extract_transcript_summary(path: &str) -> Option<(Option<String>, Option<String>)> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut query = None;
    let mut response = None;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let entry_type = entry.get("type").and_then(|v| v.as_str());
        let message = entry.get("message").unwrap_or(&Value::Null);
        let message_content = message.get("content").unwrap_or(&Value::Null);

        match entry_type {
            Some("user") => {
                if let Some(text) = text_from_content(message_content) {
                    query = Some(truncate_summary(text));
                }
            }
            Some("assistant") => {
                if let Some(text) = text_from_content(message_content) {
                    response = Some(truncate_summary(text));
                }
            }
            _ => {}
        }
    }

    if query.is_some() || response.is_some() {
        Some((query, response))
    } else {
        None
    }
}

fn run_socket_command(request: Value) {
    match socket_command_data(request) {
        Ok(Some(data)) => println!("{}", serde_json::to_string_pretty(&data).unwrap()),
        Ok(None) => {}
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn socket_command_data(request: Value) -> Result<Option<Value>, String> {
    let response = send_socket_command(request).map_err(|err| err.to_string())?;
    let ok = response.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if ok {
        Ok(response.get("data").cloned())
    } else {
        let error = response.get("error").and_then(|e| e.as_str()).unwrap_or("unknown error");
        Err(error.to_string())
    }
}

fn daemon_status_data() -> Result<Value, String> {
    socket_command_data(serde_json::json!({ "command": "daemon-status" }))?
        .ok_or_else(|| "daemon-status returned no data".to_string())
}

fn daemon_status_has_capability(status: &Value, capability: &str) -> bool {
    status
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| capabilities.iter().any(|candidate| candidate == capability))
}

fn validate_work_item_start_daemon_capabilities(
    params: &WorkItemStartArgs,
    status: &Value,
) -> Result<(), String> {
    if params.fix_ci && !daemon_status_has_capability(status, "work-item-start-fix-ci") {
        return Err(
            "Fix CI work item starts require daemon capability work-item-start-fix-ci. Restart the Roux daemon."
                .to_string(),
        );
    }
    Ok(())
}

fn run_work_item_start_command(params: WorkItemStartArgs) {
    if params.fix_ci {
        match daemon_status_data()
            .and_then(|status| validate_work_item_start_daemon_capabilities(&params, &status))
        {
            Ok(()) => {}
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    run_socket_command(build_work_item_start_request(params));
}

fn is_not_running_error(error: &str) -> bool {
    error == "Roux is not running"
}

fn daemon_timeout_from_env(key: &str, default: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn start_daemon_background() -> Result<(), String> {
    match daemon_status_data() {
        Ok(status) => {
            print_daemon_lifecycle_line("Roux daemon already running", &status);
            return Ok(());
        }
        Err(error) if is_not_running_error(&error) => {}
        Err(error) => return Err(error),
    }

    let pid = spawn_detached_daemon()?;
    let started = Instant::now();
    let timeout = daemon_timeout_from_env("ROUX_DAEMON_START_TIMEOUT_MS", Duration::from_secs(15));
    let poll_interval = Duration::from_millis(100);

    loop {
        match daemon_status_data() {
            Ok(status) => {
                print_daemon_lifecycle_line("Started roux daemon", &status);
                return Ok(());
            }
            Err(error) if is_not_running_error(&error) && started.elapsed() < timeout => {
                std::thread::sleep(poll_interval);
            }
            Err(error) if is_not_running_error(&error) => {
                return Err(format!(
                    "started roux daemon pid={pid}, but it did not become ready within {}ms",
                    timeout.as_millis()
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

fn stop_daemon_background(ignore_not_running: bool) -> Result<(), String> {
    let status = match daemon_status_data() {
        Ok(status) => status,
        Err(error) if is_not_running_error(&error) && ignore_not_running => return Ok(()),
        Err(error) if is_not_running_error(&error) => {
            println!("Roux daemon is not running");
            return Ok(());
        }
        Err(error) => return Err(error),
    };

    let _ = socket_command_data(serde_json::json!({ "command": "daemon-stop" }))?;
    let timeout = daemon_timeout_from_env("ROUX_DAEMON_STOP_TIMEOUT_MS", Duration::from_secs(5));
    let poll_interval = Duration::from_millis(100);
    let started = Instant::now();

    loop {
        match daemon_status_data() {
            Ok(_) if started.elapsed() < timeout => std::thread::sleep(poll_interval),
            Ok(_) => {
                return Err(format!(
                    "daemon-stop was acknowledged, but the daemon still responds after {}ms",
                    timeout.as_millis()
                ));
            }
            Err(_) => {
                wait_for_daemon_owner_lock_release(timeout.saturating_sub(started.elapsed()))?;
                print_daemon_lifecycle_line("Stopped roux daemon", &status);
                return Ok(());
            }
        }
    }
}

fn wait_for_daemon_owner_lock_release(timeout: Duration) -> Result<(), String> {
    let lock_path = platform::daemon_owner_lock_path();
    let started = Instant::now();
    let poll_interval = Duration::from_millis(100);
    while started.elapsed() < timeout {
        if !daemon_owner_lock_is_held(&lock_path)? {
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }
    Err(format!(
        "daemon socket stopped responding, but owner lock {} was still held after {}ms",
        lock_path.display(),
        timeout.as_millis()
    ))
}

fn daemon_owner_lock_is_held(lock_path: &Path) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!("create daemon owner lock directory {}: {err}", parent.display())
            })?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|err| format!("open daemon owner lock {}: {err}", lock_path.display()))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == -1 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(true);
            }
            return Err(format!("lock daemon owner lock {}: {err}", lock_path.display()));
        }
        let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        Ok(false)
    }

    #[cfg(not(unix))]
    {
        let _ = lock_path;
        Ok(false)
    }
}

fn clear_stale_daemon_state() -> Result<(), String> {
    match daemon_status_data() {
        Ok(status) => {
            print_daemon_lifecycle_line("Roux daemon is running; not clearing", &status);
            return Ok(());
        }
        Err(error) if is_not_running_error(&error) => {}
        Err(error) => return Err(error),
    }

    let lock_path = platform::daemon_owner_lock_path();
    let lock_holders = daemon_lock_holder_pids(&lock_path);
    for pid in &lock_holders {
        println!("Terminating stale daemon lock holder pid={pid}");
        terminate_process(*pid)?;
    }

    let mut removed = Vec::new();
    for path in [
        platform::socket_path(),
        platform::socket_addr_file_path(),
        platform::socket_auth_token_file_path(),
        lock_path,
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("remove stale daemon file {}: {err}", path.display())),
        }
    }

    if removed.is_empty() {
        println!("No stale daemon files found");
    } else {
        for path in removed {
            println!("Removed {}", path.display());
        }
    }
    Ok(())
}

fn daemon_lock_holder_pids(lock_path: &Path) -> Vec<u32> {
    #[cfg(unix)]
    {
        let Ok(output) = std::process::Command::new("lsof").arg("-t").arg(lock_path).output()
        else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        let current = std::process::id();
        return String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .filter(|pid| *pid != current)
            .collect();
    }

    #[cfg(not(unix))]
    {
        let _ = lock_path;
        Vec::new()
    }
}

fn terminate_process(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        unsafe {
            if libc::kill(pid as libc::pid_t, libc::SIGTERM) == -1 {
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!("terminate stale daemon pid={pid}: {err}"));
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if process_exists(pid) {
            unsafe {
                if libc::kill(pid as libc::pid_t, libc::SIGKILL) == -1 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() != std::io::ErrorKind::NotFound {
                        return Err(format!("kill stale daemon pid={pid}: {err}"));
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Err("daemon clear cannot terminate lock holders on this platform".to_string())
    }
}

fn process_exists(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn spawn_detached_daemon() -> Result<u32, String> {
    use std::process::Stdio;

    let exe =
        std::env::current_exe().map_err(|err| format!("resolve current roux binary: {err}"))?;
    let mut command = std::process::Command::new(exe);
    command.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    let child = command.spawn().map_err(|err| format!("spawn roux daemon: {err}"))?;
    Ok(child.id())
}

fn print_daemon_lifecycle_line(prefix: &str, status: &Value) {
    let pid = status.get("pid").and_then(|pid| pid.as_u64());
    let socket = status.get("socket").and_then(|socket| socket.as_str());
    let log_path = status.get("logPath").and_then(|log_path| log_path.as_str());

    let mut parts = vec![prefix.to_string()];
    if let Some(pid) = pid {
        parts.push(format!("pid={pid}"));
    }
    if let Some(socket) = socket {
        parts.push(format!("socket={socket}"));
    }
    if let Some(log_path) = log_path {
        parts.push(format!("log={log_path}"));
    }
    println!("{}", parts.join(" "));
}

const TCP_CONNECT_AUTH_WARNING: &str =
    "Warning: TCP daemon endpoints require an auth token; pass --auth-token or set ROUX_AUTH_TOKEN.";

fn connect_daemon_socket(socket: &str, auth_token: Option<&str>) -> Result<(), String> {
    let endpoint = parse_daemon_connect_endpoint(socket)?;
    write_private_config_file(&platform::socket_addr_file_path(), &endpoint.display_value())?;
    if let Some(auth_token) = auth_token {
        write_private_config_file(&platform::socket_auth_token_file_path(), auth_token)?;
    } else {
        remove_config_file_if_exists(&platform::socket_auth_token_file_path())?;
    }
    if let Some(warning) = daemon_connect_auth_warning(
        &endpoint,
        auth_token.is_some(),
        daemon_connect_env_auth_token_present(),
    ) {
        eprintln!("{warning}");
    }
    println!("Connected roux CLI to {}", endpoint.display_value());
    Ok(())
}

fn disconnect_daemon_socket() -> Result<(), String> {
    remove_config_file_if_exists(&platform::socket_addr_file_path())?;
    remove_config_file_if_exists(&platform::socket_auth_token_file_path())?;
    println!("Disconnected roux CLI daemon socket config");
    Ok(())
}

fn parse_daemon_connect_endpoint(socket: &str) -> Result<platform::SocketEndpoint, String> {
    let trimmed = socket.trim();
    if trimmed.is_empty() {
        return Err("daemon socket endpoint cannot be empty".to_string());
    }
    if let Some(endpoint) = platform::parse_socket_endpoint(trimmed) {
        return Ok(endpoint);
    }
    Err(format!("invalid daemon socket endpoint: {socket}"))
}

fn daemon_connect_auth_warning(
    endpoint: &platform::SocketEndpoint,
    has_cli_auth_token: bool,
    has_env_auth_token: bool,
) -> Option<&'static str> {
    match endpoint {
        platform::SocketEndpoint::Tcp(_) if !has_cli_auth_token && !has_env_auth_token => {
            Some(TCP_CONNECT_AUTH_WARNING)
        }
        _ => None,
    }
}

fn daemon_connect_env_auth_token_present() -> bool {
    ["ROUX_DAEMON_TOKEN", "ROUX_AUTH_TOKEN"]
        .iter()
        .any(|key| std::env::var(key).ok().map(|value| !value.trim().is_empty()).unwrap_or(false))
}

fn write_private_config_file(path: &PathBuf, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("create config directory {}: {err}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;

        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("roux-config");
        let tmp_path =
            path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4().simple()));

        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp_path)
                .map_err(|err| format!("write config file {}: {err}", tmp_path.display()))?;
            file.write_all(contents.as_bytes())
                .map_err(|err| format!("write config file {}: {err}", tmp_path.display()))?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600)).map_err(|err| {
                format!("set permissions on config file {}: {err}", tmp_path.display())
            })?;
            drop(file);
            std::fs::rename(&tmp_path, path).map_err(|err| {
                format!("replace config file {} with {}: {err}", path.display(), tmp_path.display())
            })?;
            Ok::<(), String>(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp_path);
        }
        result?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
            .map_err(|err| format!("write config file {}: {err}", path.display()))?;
    }

    Ok(())
}

fn remove_config_file_if_exists(path: &PathBuf) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("remove config file {}: {err}", path.display())),
    }
}

fn show_daemon_logs(lines: usize, follow: bool) -> Result<(), String> {
    let path = platform::log_dir().join("roux-daemon.log");
    if follow {
        return follow_daemon_log(&path, lines);
    }

    let content = fs::read_to_string(&path)
        .map_err(|err| format!("read daemon log {}: {err}", path.display()))?;
    let lines = tail_lines(&content, lines);
    if !lines.is_empty() {
        println!("{}", lines.join("\n"));
    }
    Ok(())
}

fn tail_lines(content: &str, count: usize) -> Vec<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].to_vec()
}

fn follow_daemon_log(path: &std::path::Path, lines: usize) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let status = std::process::Command::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg("-f")
            .arg(path)
            .status()
            .map_err(|err| format!("run tail for {}: {err}", path.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("tail exited with status {status}"))
        }
    }

    #[cfg(windows)]
    {
        let _ = (path, lines);
        Err("roux daemon logs --follow is not supported on Windows".to_string())
    }
}

/// Stream a long-lived command and print each event-bearing frame as a
/// JSON line to stdout. `ready`/`warning`/`error` frames are surfaced
/// on stderr so a downstream `jq` pipeline only sees event payloads.
fn run_streaming_command(request: Value) {
    use crate::cli_socket::stream_socket_command;
    let exit_code = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let exit_clone = exit_code.clone();

    let result = stream_socket_command(request, move |line| {
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("watch: invalid frame from server: {e}");
                return true; // keep going; one bad line shouldn't kill the watch
            }
        };
        match value.get("type").and_then(|t| t.as_str()) {
            Some("event") => {
                if let Some(event) = value.get("event") {
                    println!("{}", event);
                }
                true
            }
            Some("ready") => true,
            Some("warning") => {
                if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
                    eprintln!("watch: warning: {msg}");
                }
                true
            }
            // Server-emitted error frames (auth/dispatch failures) come
            // through as `{"ok":false,"error":"..."}` per Response::err.
            _ if value.get("ok").and_then(|v| v.as_bool()) == Some(false) => {
                let err = value.get("error").and_then(|e| e.as_str()).unwrap_or("unknown error");
                eprintln!("Error: {err}");
                exit_clone.store(1, std::sync::atomic::Ordering::SeqCst);
                false
            }
            _ => true,
        }
    });

    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
    let code = exit_code.load(std::sync::atomic::Ordering::SeqCst);
    if code != 0 {
        std::process::exit(code);
    }
}

fn insert_optional_string(
    args: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        args.insert(key.into(), Value::String(value));
    }
}

fn insert_optional_nullable_string(
    args: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<Option<String>>,
) {
    if let Some(value) = value {
        match value {
            Some(value) => {
                args.insert(key.into(), Value::String(value));
            }
            None => {
                args.insert(key.into(), Value::Null);
            }
        }
    }
}

fn insert_optional_f64(args: &mut serde_json::Map<String, Value>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        if let Some(number) = serde_json::Number::from_f64(value) {
            args.insert(key.into(), Value::Number(number));
        }
    }
}

fn build_work_item_create_request(params: WorkItemCreateArgs) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("title".into(), Value::String(params.title));
    insert_optional_string(&mut args, "body", params.body);
    insert_optional_string(&mut args, "status", params.status);
    if let Some(repo_path) = params.repo_path {
        args.insert("repoPath".into(), Value::String(resolve_path(&repo_path)));
    }
    insert_optional_string(&mut args, "agentProfile", params.agent_profile);
    insert_optional_string(&mut args, "baseBranch", params.base_branch);
    if let Some(worktree_path) = params.worktree_path {
        args.insert("worktreePath".into(), Value::String(resolve_path(&worktree_path)));
    }
    insert_optional_string(&mut args, "branch", params.branch);
    if params.fetch_first {
        args.insert("fetchFirst".into(), Value::Bool(true));
    }
    insert_optional_string(&mut args, "projectId", params.project);
    insert_optional_string(&mut args, "parentId", params.parent);
    insert_optional_f64(&mut args, "sortOrder", params.sort_order);
    serde_json::json!({
        "command": "work-item-create",
        "args": Value::Object(args),
    })
}

fn build_work_item_update_request(params: WorkItemUpdateArgs) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), Value::String(params.id));
    args.insert("title".into(), Value::String(params.title));
    insert_optional_string(&mut args, "body", params.body);
    insert_optional_string(&mut args, "status", params.status);
    if let Some(repo_path) = params.repo_path {
        args.insert("repoPath".into(), Value::String(resolve_path(&repo_path)));
    }
    insert_optional_string(&mut args, "agentProfile", params.agent_profile);
    insert_optional_string(&mut args, "baseBranch", params.base_branch);
    if let Some(worktree_path) = params.worktree_path {
        args.insert("worktreePath".into(), Value::String(resolve_path(&worktree_path)));
    }
    insert_optional_nullable_string(&mut args, "branch", params.branch);
    if let Some(fetch_first) = params.fetch_first {
        args.insert("fetchFirst".into(), Value::Bool(fetch_first));
    }
    insert_optional_string(&mut args, "projectId", params.project);
    insert_optional_string(&mut args, "parentId", params.parent);
    insert_optional_f64(&mut args, "sortOrder", params.sort_order);
    serde_json::json!({
        "command": "work-item-update",
        "args": Value::Object(args),
    })
}

fn build_work_item_start_request(params: WorkItemStartArgs) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), Value::String(params.id));
    insert_optional_string(&mut args, "profile", params.profile);
    if let Some(repo_path) = params.repo_path {
        args.insert("repoPath".into(), Value::String(resolve_path(&repo_path)));
    }
    insert_optional_string(&mut args, "name", params.name);
    if let Some(worktree_path) = params.worktree_path {
        args.insert("worktreePath".into(), Value::String(resolve_path(&worktree_path)));
    }
    insert_optional_string(&mut args, "branch", params.branch);
    insert_optional_string(&mut args, "base", params.base);
    if params.fetch_first {
        args.insert("fetchFirst".into(), Value::Bool(true));
    }
    if params.force_start {
        args.insert("forceStart".into(), Value::Bool(true));
    }
    if params.fix_ci {
        args.insert("fixCi".into(), Value::Bool(true));
    }
    serde_json::json!({
        "command": "work-item-start",
        "args": Value::Object(args),
    })
}

fn build_work_item_review_request(params: WorkItemReviewRequestArgs) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("runId".into(), Value::String(params.run_id));
    insert_optional_string(&mut args, "summary", params.summary);
    if !params.tests.is_empty() {
        args.insert(
            "tests".into(),
            Value::Array(params.tests.into_iter().map(Value::String).collect()),
        );
    }
    if !params.changed_files.is_empty() {
        args.insert(
            "changedFiles".into(),
            Value::Array(params.changed_files.into_iter().map(Value::String).collect()),
        );
    }
    serde_json::json!({
        "command": "work-item-review-request",
        "args": Value::Object(args),
    })
}

fn build_work_item_review_request_changes(
    target: String,
    note: String,
    status: Option<String>,
) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), Value::String(target));
    args.insert("note".into(), Value::String(note));
    insert_optional_string(&mut args, "status", status);
    serde_json::json!({
        "command": "work-item-review-request-changes",
        "args": Value::Object(args),
    })
}

fn build_work_item_review_accept_request(id: String) -> Value {
    serde_json::json!({
        "command": "work-item-review-accept",
        "args": { "id": id },
    })
}

fn build_work_item_plan_request(params: WorkItemPlanArgs) -> Value {
    let mut args = serde_json::Map::new();
    args.insert("id".into(), Value::String(params.id));
    insert_optional_string(&mut args, "profile", params.profile);
    if let Some(repo_path) = params.repo_path {
        args.insert("repoPath".into(), Value::String(resolve_path(&repo_path)));
    }
    insert_optional_string(&mut args, "name", params.name);
    if let Some(worktree_path) = params.worktree_path {
        args.insert("worktreePath".into(), Value::String(resolve_path(&worktree_path)));
    }
    if params.replace_active {
        args.insert("replaceActive".into(), Value::Bool(true));
    }
    serde_json::json!({
        "command": "work-item-plan",
        "args": Value::Object(args),
    })
}

fn parse_work_item_decision_options(options: Vec<String>) -> Result<Value, String> {
    let mut parsed = Vec::with_capacity(options.len());
    for raw in options {
        let (value, label) = raw.split_once('=').unwrap_or((raw.as_str(), raw.as_str()));
        let value = value.trim();
        let label = label.trim();
        if value.is_empty() || label.is_empty() {
            return Err("--option requires a non-empty value and label".to_string());
        }
        parsed.push(serde_json::json!({ "value": value, "label": label }));
    }
    Ok(Value::Array(parsed))
}

fn build_work_item_decision_create_request(
    params: WorkItemDecisionCreateArgs,
) -> Result<Value, String> {
    let has_timeout = params.timeout_at.is_some()
        || params.timeout_seconds.is_some()
        || params.timeout_ms.is_some();
    if has_timeout && params.default_value.is_none() {
        return Err("decision timeout requires --default-value".to_string());
    }
    let mut args = serde_json::Map::new();
    args.insert("runId".into(), Value::String(params.run_id));
    args.insert("question".into(), Value::String(params.question));
    args.insert("options".into(), parse_work_item_decision_options(params.options)?);
    insert_optional_string(&mut args, "defaultValue", params.default_value);
    if let Some(timeout_at) = params.timeout_at {
        args.insert("timeoutAt".into(), Value::Number(timeout_at.into()));
    }
    if let Some(timeout_seconds) = params.timeout_seconds {
        args.insert("timeoutSeconds".into(), Value::Number(timeout_seconds.into()));
    }
    if let Some(timeout_ms) = params.timeout_ms {
        args.insert("timeoutMs".into(), Value::Number(timeout_ms.into()));
    }
    Ok(serde_json::json!({
        "command": "work-item-decision-create",
        "args": Value::Object(args),
    }))
}

fn build_work_item_import_request(
    path: Option<String>,
    items_json: Option<String>,
) -> Result<Value, String> {
    let mut args = serde_json::Map::new();
    match (path, items_json) {
        (Some(path), None) => {
            args.insert("path".into(), Value::String(resolve_path(&path)));
        }
        (None, Some(items_json)) => {
            let items: Value = serde_json::from_str(&items_json)
                .map_err(|err| format!("invalid --items-json: {err}"))?;
            if !items.is_array() {
                return Err("--items-json must be a JSON array".to_string());
            }
            args.insert("items".into(), items);
        }
        (None, None) => return Err("work-item import requires --path or --items-json".to_string()),
        (Some(_), Some(_)) => {
            return Err("work-item import accepts only one of --path or --items-json".to_string())
        }
    }
    Ok(serde_json::json!({
        "command": "work-item-import",
        "args": Value::Object(args),
    }))
}

fn build_document_attach_request(params: DocumentAttachArgs) -> Result<Value, String> {
    let (target_kind, target_id) = match (params.session, params.work_item) {
        (Some(session), None) => ("session", session),
        (None, Some(work_item)) => ("workItem", work_item),
        _ => return Err("document attach requires exactly one of --session or --work-item".into()),
    };
    let (content_kind, content, source_path) = match (params.text, params.file) {
        (Some(text), None) => ("text", text, None),
        (None, Some(path)) => {
            let resolved = resolve_path(&path);
            let content = fs::read_to_string(&resolved)
                .map_err(|err| format!("read document file {resolved}: {err}"))?;
            ("file", content, Some(resolved))
        }
        _ => return Err("document attach requires exactly one of --text or --file".into()),
    };
    let mut args = serde_json::Map::new();
    args.insert("targetKind".into(), Value::String(target_kind.into()));
    args.insert("targetId".into(), Value::String(target_id));
    args.insert("contentKind".into(), Value::String(content_kind.into()));
    args.insert("content".into(), Value::String(content));
    insert_optional_string(&mut args, "title", params.title);
    insert_optional_string(&mut args, "mimeType", params.mime_type);
    insert_optional_string(&mut args, "sourcePath", source_path);
    Ok(serde_json::json!({
        "command": "document-attach",
        "args": Value::Object(args),
    }))
}

fn build_document_list_request(params: DocumentListArgs) -> Result<Value, String> {
    let mut args = serde_json::Map::new();
    match (params.session, params.work_item) {
        (Some(session), None) => {
            args.insert("targetKind".into(), Value::String("session".into()));
            args.insert("targetId".into(), Value::String(session));
        }
        (None, Some(work_item)) => {
            args.insert("targetKind".into(), Value::String("workItem".into()));
            args.insert("targetId".into(), Value::String(work_item));
        }
        (None, None) => {}
        _ => return Err("document list accepts only one of --session or --work-item".into()),
    }
    Ok(serde_json::json!({
        "command": "document-list",
        "args": Value::Object(args),
    }))
}

fn build_document_get_request(id: String) -> Value {
    serde_json::json!({
        "command": "document-get",
        "args": { "id": id },
    })
}

fn exit_with_input_error(message: impl std::fmt::Display) -> ! {
    eprintln!("Error: {message}");
    std::process::exit(2);
}

fn handle_document(action: DocumentAction) {
    match action {
        DocumentAction::Attach(params) => {
            let request = match build_document_attach_request(params) {
                Ok(request) => request,
                Err(err) => exit_with_input_error(err),
            };
            run_socket_command(request);
        }
        DocumentAction::List(params) => {
            let request = match build_document_list_request(params) {
                Ok(request) => request,
                Err(err) => exit_with_input_error(err),
            };
            run_socket_command(request);
        }
        DocumentAction::Get { id } => run_socket_command(build_document_get_request(id)),
    }
}

fn handle_work_item(action: WorkItemAction) {
    match action {
        WorkItemAction::List { project, include_archived } => {
            let mut args = serde_json::Map::new();
            insert_optional_string(&mut args, "projectId", project);
            if include_archived {
                args.insert("includeArchived".into(), Value::Bool(true));
            }
            run_socket_command(serde_json::json!({
                "command": "work-item-list",
                "args": Value::Object(args),
            }));
        }
        WorkItemAction::Create(params) => {
            run_socket_command(build_work_item_create_request(params))
        }
        WorkItemAction::Update(params) => {
            run_socket_command(build_work_item_update_request(params))
        }
        WorkItemAction::Move { id, status, sort_order } => {
            let mut args = serde_json::Map::new();
            args.insert("id".into(), Value::String(id));
            args.insert("status".into(), Value::String(status));
            insert_optional_f64(&mut args, "sortOrder", sort_order);
            run_socket_command(serde_json::json!({
                "command": "work-item-move",
                "args": Value::Object(args),
            }));
        }
        WorkItemAction::Delete { id } => {
            run_socket_command(serde_json::json!({
                "command": "work-item-delete",
                "args": { "id": id },
            }));
        }
        WorkItemAction::Archive { id } => {
            run_socket_command(serde_json::json!({
                "command": "work-item-archive",
                "args": { "id": id },
            }));
        }
        WorkItemAction::Restore { id } => {
            run_socket_command(serde_json::json!({
                "command": "work-item-restore",
                "args": { "id": id },
            }));
        }
        WorkItemAction::Plan(params) => run_socket_command(build_work_item_plan_request(params)),
        WorkItemAction::Start(params) => run_work_item_start_command(params),
        WorkItemAction::Review { action } => match action {
            WorkItemReviewAction::Request { run_id, summary, tests, changed_files } => {
                run_socket_command(build_work_item_review_request(WorkItemReviewRequestArgs {
                    run_id,
                    summary,
                    tests,
                    changed_files,
                }));
            }
            WorkItemReviewAction::RequestChanges { target, note, status } => {
                run_socket_command(build_work_item_review_request_changes(target, note, status));
            }
            WorkItemReviewAction::Accept { id } => {
                run_socket_command(build_work_item_review_accept_request(id));
            }
        },
        WorkItemAction::Accept { id } => {
            run_socket_command(build_work_item_review_accept_request(id));
        }
        WorkItemAction::Runs { work_item } => {
            let mut args = serde_json::Map::new();
            insert_optional_string(&mut args, "workItemId", work_item);
            run_socket_command(serde_json::json!({
                "command": "work-item-runs-list",
                "args": Value::Object(args),
            }));
        }
        WorkItemAction::Events { run_id } => {
            run_socket_command(serde_json::json!({
                "command": "work-item-run-events",
                "args": { "runId": run_id },
            }));
        }
        WorkItemAction::Stop { run_id } => {
            run_socket_command(serde_json::json!({
                "command": "work-item-run-stop",
                "args": { "runId": run_id },
            }));
        }
        WorkItemAction::Decision { action } => match action {
            WorkItemDecisionAction::Create(params) => {
                let request = match build_work_item_decision_create_request(params) {
                    Ok(request) => request,
                    Err(err) => exit_with_input_error(err),
                };
                run_socket_command(request);
            }
            WorkItemDecisionAction::List { work_item } => {
                let mut args = serde_json::Map::new();
                insert_optional_string(&mut args, "workItemId", work_item);
                run_socket_command(serde_json::json!({
                    "command": "work-item-decisions-list",
                    "args": Value::Object(args),
                }));
            }
            WorkItemDecisionAction::Resolve { id, value, resolved_by } => {
                let mut args = serde_json::Map::new();
                args.insert("id".into(), Value::String(id));
                args.insert("value".into(), Value::String(value));
                insert_optional_string(&mut args, "resolvedBy", resolved_by);
                run_socket_command(serde_json::json!({
                    "command": "work-item-decision-resolve",
                    "args": Value::Object(args),
                }));
            }
        },
        WorkItemAction::Import { path, items_json } => {
            let request = match build_work_item_import_request(path, items_json) {
                Ok(request) => request,
                Err(err) => exit_with_input_error(err),
            };
            run_socket_command(request);
        }
        WorkItemAction::Watch => {
            run_streaming_command(serde_json::json!({
                "command": "work-item-events",
                "args": {},
            }));
        }
    }
}

fn handle_hook(status: &str) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }

    let data: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return,
    };

    if status == "idle" && data.get("stop_hook_active").and_then(|v| v.as_bool()).unwrap_or(false) {
        return;
    }

    let sid = match data.get("session_id").and_then(|s| s.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return,
    };

    let cwd = data.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let mut out = serde_json::json!({
        "status": status,
        "provider": "claude",
        // Provider-agnostic key; the old `claude_session_id` name was
        // confusing once we picked up non-Claude agents. The watcher still
        // accepts the legacy key so an older roux-cli shim paired with a
        // newer desktop binary keeps working.
        "provider_session_id": sid,
        "cwd": cwd,
        "timestamp": timestamp,
    });

    // Pane-scoped routing fields. Present when the agent was launched inside
    // a Roux-managed PTY (which injects both env vars unconditionally). Absent
    // for legacy or external installs — status_watcher falls back to cwd
    // matching in that case, for notifications only.
    if let Some(roux_sid) = get_session_id() {
        out["roux_session_id"] = Value::String(roux_sid);
    }
    if let Some(pane) = get_pane_id() {
        out["roux_pane_id"] = Value::String(pane);
    }
    if let Some(work_item_id) = get_work_item_id() {
        out["roux_work_item_id"] = Value::String(work_item_id);
    }
    if let Some(run_id) = get_work_item_run_id() {
        out["roux_work_item_run_id"] = Value::String(run_id);
    }

    if status == "attention" {
        if let Some(tn) = data.get("tool_name") {
            out["tool_name"] = tn.clone();
        }
        if let Some(ti) = data.get("tool_input") {
            out["tool_input"] = ti.clone();
        }
        if let Some(msg) = data.get("message") {
            out["message"] = msg.clone();
        }
    }

    if status == "idle" {
        if let Some(path) =
            data.get("transcript_path").and_then(|s| s.as_str()).filter(|s| !s.is_empty())
        {
            out["transcript_path"] = Value::String(path.to_string());
            if let Some((query, response)) = extract_transcript_summary(path) {
                if let Some(query) = query {
                    out["query"] = Value::String(query);
                }
                if let Some(response) = response {
                    out["response"] = Value::String(response);
                }
            }
        }
    }

    if status == "error" {
        if let Some(et) = data.get("error_type") {
            out["error_type"] = et.clone();
        }
        if let Some(em) = data.get("error_message") {
            out["error_message"] = em.clone();
        }
    }

    let dir = status_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", sid));
    let json = serde_json::to_string(&out).unwrap_or_default();
    let _ = fs::write(path, json);
}

fn handle_hook_action(action: HookAction) {
    match action {
        HookAction::Working => handle_hook("working"),
        HookAction::Idle => handle_hook("idle"),
        HookAction::Attention => handle_hook("attention"),
        HookAction::Error => handle_hook("error"),
        HookAction::Disconnected => handle_hook("disconnected"),
        HookAction::Show(HookShowArgs { repo_path }) => {
            let mut args = serde_json::Map::new();
            if let Some(path) = repo_path {
                args.insert("repo_path".into(), Value::String(path));
            }
            run_socket_command(serde_json::json!({
                "command": "hook-show",
                "args": Value::Object(args),
            }));
        }
        HookAction::Run(args) => {
            let HookRunArgs {
                event,
                repo_path,
                worktree_path,
                branch,
                session,
                project,
                task,
                scope,
                provider,
                extra,
            } = *args;
            let mut args = serde_json::Map::new();
            args.insert("event".into(), Value::String(event));
            if let Some(path) = repo_path {
                args.insert("repoPath".into(), Value::String(path));
            }
            if let Some(path) = worktree_path {
                args.insert("worktreePath".into(), Value::String(path));
            }
            if let Some(branch) = branch {
                args.insert("branch".into(), Value::String(branch));
            }
            if let Some(session) = session.or_else(get_session_id) {
                args.insert("sessionId".into(), Value::String(session));
            }
            if let Some(project) = project {
                args.insert("projectId".into(), Value::String(project));
            }
            if let Some(task) = task {
                args.insert("taskId".into(), Value::String(task));
            }
            if let Some(scope) = scope {
                args.insert("scope".into(), Value::String(scope));
            }
            if let Some(provider) = provider {
                args.insert("provider".into(), Value::String(provider));
            }
            if !extra.is_empty() {
                args.insert(
                    "args".into(),
                    Value::Array(extra.into_iter().map(Value::String).collect()),
                );
            }
            run_socket_command(serde_json::json!({
                "command": "hook-run",
                "args": Value::Object(args),
            }));
        }
    }
}

fn show_status() {
    let dir = status_dir();
    if !dir.exists() {
        println!("No status files found");
        return;
    }

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
        .collect();

    if entries.is_empty() {
        println!("No status files found");
        return;
    }

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let content = fs::read_to_string(entry.path()).unwrap_or_default();
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("?");
            let cwd = data.get("cwd").and_then(|s| s.as_str()).unwrap_or("?");
            let sid = entry.path().file_stem().unwrap().to_string_lossy().to_string();
            println!("{sid}  status={status}  cwd={cwd}");
        }
    }
}

fn clear_status() {
    let dir = status_dir();
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut count = 0;
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                let _ = fs::remove_file(entry.path());
                count += 1;
            }
        }
        println!("Cleared {} status file(s)", count);
    } else {
        println!("No status directory found");
    }
}

fn main() {
    paths::migrate_legacy_config_dir();

    let cli = Cli::parse();
    match cli.command {
        Commands::Hook { action } => handle_hook_action(action),
        Commands::Install { action } => handle_install_action(action),
        Commands::Status => show_status(),
        Commands::Clear => clear_status(),
        Commands::Mcp => {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(e) => {
                    eprintln!("Error: failed to start MCP runtime: {}", e);
                    std::process::exit(1);
                }
            };
            if let Err(e) = runtime.block_on(mcp::run_stdio_server()) {
                eprintln!("Error: MCP server exited: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Daemon { action } => match action {
            None => {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        eprintln!("Error: failed to start daemon runtime: {}", e);
                        std::process::exit(1);
                    }
                };
                if let Err(e) = runtime.block_on(daemon::run()) {
                    eprintln!("Error: daemon exited: {}", e);
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Status) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-status",
                }));
            }
            Some(DaemonAction::Start) => {
                if let Err(e) = start_daemon_background() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Stop) => {
                if let Err(e) = stop_daemon_background(false) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Restart) => {
                if let Err(e) = stop_daemon_background(true) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                if let Err(e) = start_daemon_background() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Clear) => {
                if let Err(e) = clear_stale_daemon_state() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Logs { lines, follow }) => {
                if let Err(e) = show_daemon_logs(lines, follow) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Connect { socket, auth_token }) => {
                if let Err(e) = connect_daemon_socket(&socket, auth_token.as_deref()) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Disconnect) => {
                if let Err(e) = disconnect_daemon_socket() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            Some(DaemonAction::Run { command, working_dir }) => {
                let mut args = serde_json::Map::new();
                args.insert("command".into(), Value::String(command));
                if let Some(working_dir) = working_dir {
                    args.insert("workingDir".into(), Value::String(resolve_path(&working_dir)));
                }
                run_socket_command(serde_json::json!({
                    "command": "daemon-process-start",
                    "args": args,
                }));
            }
            Some(DaemonAction::Output { id, max_bytes }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-process-output",
                    "args": {
                        "id": id,
                        "maxBytes": max_bytes,
                    },
                }));
            }
            Some(DaemonAction::Processes) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-process-list",
                }));
            }
            Some(DaemonAction::Kill { id }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-process-kill",
                    "args": { "id": id },
                }));
            }
            Some(DaemonAction::PtyRun {
                command,
                working_dir,
                session_id,
                pane_id,
                profile,
                cols,
                rows,
            }) => {
                let mut args = serde_json::Map::new();
                args.insert("command".into(), Value::String(command));
                if let Some(working_dir) = working_dir {
                    args.insert("workingDir".into(), Value::String(resolve_path(&working_dir)));
                }
                if let Some(session_id) = session_id {
                    args.insert("sessionId".into(), Value::String(session_id));
                }
                if let Some(pane_id) = pane_id {
                    args.insert("paneId".into(), Value::String(pane_id));
                }
                if let Some(profile) = profile {
                    args.insert("profile".into(), Value::String(profile));
                }
                if let (Some(cols), Some(rows)) = (cols, rows) {
                    args.insert("initialSize".into(), serde_json::json!([cols, rows]));
                }
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-spawn-task",
                    "args": args,
                }));
            }
            Some(DaemonAction::PtyOutput { id, max_bytes }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-output",
                    "args": {
                        "id": id,
                        "maxBytes": max_bytes,
                    },
                }));
            }
            Some(DaemonAction::Ptys) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-list",
                }));
            }
            Some(DaemonAction::PtyWrite { id, data }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-write",
                    "args": {
                        "id": id,
                        "data": data,
                    },
                }));
            }
            Some(DaemonAction::PtyResize { id, cols, rows }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-resize",
                    "args": {
                        "id": id,
                        "cols": cols,
                        "rows": rows,
                    },
                }));
            }
            Some(DaemonAction::PtyKill { id }) => {
                run_socket_command(serde_json::json!({
                    "command": "daemon-pty-kill",
                    "args": { "id": id },
                }));
            }
            Some(DaemonAction::Watches) => {
                run_socket_command(serde_json::json!({
                    "command": "watch-list",
                }));
            }
        },

        Commands::WorkItem { action } => handle_work_item(action),

        Commands::App { path } => {
            let resolved = resolve_path(&path);
            run_socket_command(serde_json::json!({
                "command": "app-open",
                "args": { "path": resolved },
            }));
        }

        Commands::Alias { action } => match action {
            AliasAction::Set { alias, session, project, force } => {
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                if let Some(s) = session {
                    args.insert("session_id".into(), Value::String(s));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if force {
                    args.insert("force".into(), Value::Bool(true));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-set",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            AliasAction::Unset { alias, project } => {
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-unset",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            AliasAction::Claim { alias, project, steal } => {
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if steal {
                    args.insert("steal".into(), Value::Bool(true));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-claim",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            AliasAction::List { project, global, only_unbound } => {
                let mut args = serde_json::Map::new();
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if global {
                    args.insert("global".into(), Value::Bool(true));
                }
                if only_unbound {
                    args.insert("only_unbound".into(), Value::Bool(true));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-list",
                    "args": Value::Object(args),
                }));
            }
            AliasAction::Get { alias, project } => {
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-get",
                    "args": Value::Object(args),
                }));
            }
            AliasAction::Whoami { session } => {
                let effective_session = session.or_else(get_session_id);
                run_socket_command(serde_json::json!({
                    "command": "alias-whoami",
                    "session_id": effective_session,
                    "args": {},
                }));
            }
            AliasAction::AddMember { alias, pane, project } => {
                // Fail fast locally rather than burning a socket round-trip
                // on a request the backend will reject for missing pane_id.
                let pane_id = pane.or_else(get_pane_id).unwrap_or_else(|| {
                    eprintln!("Error: alias add-member requires --pane <id> or $ROUX_PANE_ID");
                    std::process::exit(2);
                });
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                args.insert("pane_id".into(), Value::String(pane_id));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-add-member",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            AliasAction::RemoveMember { alias, pane, project } => {
                let pane_id = pane.or_else(get_pane_id).unwrap_or_else(|| {
                    eprintln!("Error: alias remove-member requires --pane <id> or $ROUX_PANE_ID");
                    std::process::exit(2);
                });
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                args.insert("pane_id".into(), Value::String(pane_id));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-remove-member",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            AliasAction::Mode { alias, mode, project } => {
                let mut args = serde_json::Map::new();
                args.insert("alias".into(), Value::String(alias));
                args.insert("mode".into(), Value::String(mode));
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "alias-mode",
                    "session_id": get_session_id(),
                    "args": Value::Object(args),
                }));
            }
        },

        Commands::Mailbox { action } => match action {
            MailboxAction::Post {
                body,
                to,
                topic,
                subject,
                kind,
                project,
                correlation_id,
                from,
            } => {
                // Fail fast locally rather than burning a socket round-trip
                // on a malformed post.
                if to.is_none() && topic.is_none() {
                    eprintln!("Error: mailbox post requires at least one of --to or --topic");
                    std::process::exit(2);
                }
                let mut args = serde_json::Map::new();
                args.insert("body".into(), Value::String(body));
                if let Some(v) = to {
                    args.insert("to".into(), Value::String(v));
                }
                if let Some(v) = topic {
                    args.insert("topic".into(), Value::String(v));
                }
                if let Some(v) = subject {
                    args.insert("subject".into(), Value::String(v));
                }
                if let Some(v) = kind {
                    args.insert("kind".into(), Value::String(v));
                }
                if let Some(v) = project {
                    args.insert("project_id".into(), Value::String(v));
                }
                if let Some(v) = correlation_id {
                    args.insert("correlation_id".into(), Value::String(v));
                }
                if let Some(v) = from {
                    args.insert("from".into(), Value::String(v));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-post",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Peek { alias, unread, project, global, limit } => {
                run_socket_command(serde_json::json!({
                    "command": "mailbox-peek",
                    "session_id": get_session_id(),
                    "args": build_mailbox_recv_args(alias, project, global, Some(unread), limit),
                }));
            }
            MailboxAction::Read { alias, ack, project, global, limit } => {
                let mut args = build_mailbox_recv_args(alias, project, global, None, limit);
                if ack {
                    args["ack"] = Value::Bool(true);
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-read",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": args,
                }));
            }
            MailboxAction::Ack { event_id, result, alias } => {
                let mut args = serde_json::Map::new();
                args.insert("event_id".into(), Value::String(event_id));
                if let Some(r) = result {
                    args.insert("result".into(), Value::String(r));
                }
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-ack",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Count { alias, project, global } => {
                run_socket_command(serde_json::json!({
                    "command": "mailbox-count",
                    "session_id": get_session_id(),
                    "args": build_mailbox_recv_args(alias, project, global, None, None),
                }));
            }
            MailboxAction::Clear { alias, project, global } => {
                run_socket_command(serde_json::json!({
                    "command": "mailbox-clear",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": build_mailbox_recv_args(alias, project, global, None, None),
                }));
            }
            MailboxAction::Reply { event_id, body, subject, kind } => {
                let mut args = serde_json::Map::new();
                args.insert("event_id".into(), Value::String(event_id));
                args.insert("body".into(), Value::String(body));
                if let Some(s) = subject {
                    args.insert("subject".into(), Value::String(s));
                }
                if let Some(k) = kind {
                    args.insert("kind".into(), Value::String(k));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-reply",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Sent { to, sender, limit } => {
                let mut args = serde_json::Map::new();
                if let Some(t) = to {
                    args.insert("to".into(), Value::String(t));
                }
                if let Some(s) = sender {
                    args.insert("sender".into(), Value::String(s));
                }
                if let Some(n) = limit {
                    args.insert("limit".into(), Value::Number(n.into()));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-sent",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Watch { alias, ack, no_backlog, project, global } => {
                let mut args = serde_json::Map::new();
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if global {
                    args.insert("global".into(), Value::Bool(true));
                }
                if ack {
                    args.insert("ack".into(), Value::Bool(true));
                }
                if no_backlog {
                    args.insert("backlog".into(), Value::Bool(false));
                }
                run_streaming_command(serde_json::json!({
                    "command": "mailbox-watch",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Unsend { event_id, alias } => {
                let mut args = serde_json::Map::new();
                args.insert("event_id".into(), Value::String(event_id));
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-retract",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            MailboxAction::Dismiss { event_id, alias } => {
                let mut args = serde_json::Map::new();
                args.insert("event_id".into(), Value::String(event_id));
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                run_socket_command(serde_json::json!({
                    "command": "mailbox-dismiss",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
        },

        Commands::Bus { action } => match action {
            BusAction::Publish { topic, body, kind, project, subject } => {
                let mut args = serde_json::Map::new();
                args.insert("topic".into(), Value::String(topic));
                args.insert("body".into(), Value::String(body));
                if let Some(k) = kind {
                    args.insert("kind".into(), Value::String(k));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if let Some(s) = subject {
                    args.insert("subject".into(), Value::String(s));
                }
                run_socket_command(serde_json::json!({
                    "command": "bus-publish",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            BusAction::Tail { topic, project, global, limit } => {
                let mut args = serde_json::Map::new();
                if let Some(t) = topic {
                    args.insert("topic".into(), Value::String(t));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if global {
                    args.insert("global".into(), Value::Bool(true));
                }
                if let Some(n) = limit {
                    args.insert("limit".into(), Value::Number(n.into()));
                }
                run_socket_command(serde_json::json!({
                    "command": "bus-tail",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            BusAction::Subscribe { pattern, alias, project } => {
                let mut args = serde_json::Map::new();
                args.insert("pattern".into(), Value::String(pattern));
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "bus-subscribe",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            BusAction::Unsubscribe { id } => {
                let mut args = serde_json::Map::new();
                args.insert("id".into(), Value::String(id));
                run_socket_command(serde_json::json!({
                    "command": "bus-unsubscribe",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
            BusAction::Subscriptions { alias, project, global } => {
                let mut args = serde_json::Map::new();
                if let Some(a) = alias {
                    args.insert("alias".into(), Value::String(a));
                }
                if let Some(p) = project {
                    args.insert("project_id".into(), Value::String(p));
                }
                if global {
                    args.insert("global".into(), Value::Bool(true));
                }
                run_socket_command(serde_json::json!({
                    "command": "bus-subscriptions",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": Value::Object(args),
                }));
            }
        },

        Commands::Split { direction } => {
            run_socket_command(serde_json::json!({
                "command": "split",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": { "direction": direction },
            }));
        }

        Commands::Session { action } => match action {
            SessionAction::Create {
                name,
                working_dir,
                worktree_branch,
                from,
                profile,
                flags,
                prompt,
            } => {
                // Default working_dir to the current directory when the caller
                // is not already inside a Roux session (which lets the backend
                // inherit repo_root from $ROUX_SESSION_ID). With --worktree-branch
                // the working_dir still becomes the repo to branch off.
                let working_dir = match working_dir {
                    Some(d) => Some(d),
                    None if get_session_id().is_some() => None,
                    None => Some(resolve_path(".")),
                };
                let mut args = serde_json::Map::new();
                if let Some(n) = name {
                    args.insert("name".into(), Value::String(n));
                }
                if let Some(d) = working_dir {
                    args.insert("working_dir".into(), Value::String(d));
                }
                if let Some(b) = worktree_branch {
                    args.insert("worktree_branch".into(), Value::String(b));
                }
                if let Some(sp) = from {
                    args.insert("start_point".into(), Value::String(sp));
                }
                if let Some(p) = profile {
                    args.insert("profile".into(), Value::String(p));
                }
                if !flags.is_empty() {
                    args.insert(
                        "flags".into(),
                        Value::Array(flags.into_iter().map(Value::String).collect()),
                    );
                }
                if let Some(p) = prompt {
                    args.insert("prompt".into(), Value::String(p));
                }
                run_socket_command(serde_json::json!({
                    "command": "session-create",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": args,
                }));
            }
            SessionAction::Send { text, session, pane, pane_type, no_enter } => {
                let explicit_pane = pane.is_some();
                // When --pane-type is given without an explicit --pane, suppress the
                // env-inherited $ROUX_PANE_ID so the server's pane-type resolver runs.
                let env_pane =
                    if pane_type.is_some() && !explicit_pane { None } else { get_pane_id() };
                let (session_id, pane_id) =
                    resolve_target(session, pane, get_session_id(), env_pane);
                let mut args = serde_json::json!({ "text": text, "enter": !no_enter });
                if !explicit_pane {
                    if let Some(pt) = pane_type {
                        args["pane_type"] = serde_json::Value::String(pt);
                    }
                }
                run_socket_command(serde_json::json!({
                    "command": "send",
                    "session_id": session_id,
                    "pane_id": pane_id,
                    "args": args,
                }));
            }
            SessionAction::Poll { session } => {
                run_socket_command(serde_json::json!({
                    "command": "session-poll",
                    "session_id": session.or_else(get_session_id),
                }));
            }
            SessionAction::List => {
                run_socket_command(serde_json::json!({
                    "command": "session-list",
                }));
            }
            SessionAction::Rename { name, session } => {
                run_socket_command(serde_json::json!({
                    "command": "session-rename",
                    "session_id": session.or_else(get_session_id),
                    "args": { "name": name },
                }));
            }
            SessionAction::Kill { session } => {
                run_socket_command(serde_json::json!({
                    "command": "session-kill",
                    "session_id": session.or_else(get_session_id),
                }));
            }
            SessionAction::Panes { action } => match action {
                PaneAction::List { session } => {
                    run_socket_command(serde_json::json!({
                        "command": "session-panes-list",
                        "session_id": session.or_else(get_session_id),
                    }));
                }
                PaneAction::Create { session, profile, direction, working_dir } => {
                    let mut args = serde_json::Map::new();
                    args.insert("direction".into(), Value::String(direction));
                    if let Some(p) = profile {
                        args.insert("profile".into(), Value::String(p));
                    }
                    if let Some(d) = working_dir {
                        args.insert("working_dir".into(), Value::String(d));
                    }
                    run_socket_command(serde_json::json!({
                        "command": "session-panes-create",
                        "session_id": session.or_else(get_session_id),
                        "args": args,
                    }));
                }
            },
        },

        Commands::Shell { working_dir } => {
            let mut args = serde_json::Map::new();
            if let Some(d) = working_dir {
                args.insert("working_dir".into(), Value::String(d));
            }
            run_socket_command(serde_json::json!({
                "command": "shell",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": args,
            }));
        }

        Commands::Focus { pane, session } => {
            let (session_id, pane_id) =
                resolve_target(session, pane, get_session_id(), get_pane_id());
            run_socket_command(serde_json::json!({
                "command": "focus",
                "session_id": session_id,
                "pane_id": pane_id,
            }));
        }

        Commands::Run { command, working_dir } => {
            let mut args = serde_json::json!({ "command": command });
            if let Some(d) = working_dir {
                args["working_dir"] = Value::String(d);
            }
            run_socket_command(serde_json::json!({
                "command": "run",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": args,
            }));
        }

        Commands::Attach { target, session, max_bytes, no_input } => {
            match attach::run(attach::AttachOptions { target, session, max_bytes, no_input }) {
                Ok(0) => {}
                Ok(code) => std::process::exit(code),
                Err(err) => {
                    eprintln!("Error: {err}");
                    std::process::exit(1);
                }
            }
        }

        Commands::Notify { title, body, subtitle, level, session, cwd, source, json } => {
            let mut payload = if json {
                let mut input = String::new();
                if std::io::stdin().read_to_string(&mut input).is_err() {
                    eprintln!("Error: failed to read JSON from stdin");
                    std::process::exit(1);
                }
                match serde_json::from_str::<Value>(&input) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error: invalid JSON: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                let Some(title) = title else {
                    eprintln!("Error: --title is required unless --json is used");
                    std::process::exit(1);
                };
                let mut obj = serde_json::Map::new();
                obj.insert("title".into(), Value::String(title));
                obj.insert("level".into(), Value::String(level));
                if let Some(b) = body {
                    obj.insert("body".into(), Value::String(b));
                }
                if let Some(s) = subtitle {
                    obj.insert("subtitle".into(), Value::String(s));
                }
                if let Some(s) = source {
                    obj.insert("source".into(), Value::String(s));
                }
                Value::Object(obj)
            };

            // Resolve the session to attach to:
            //   1. explicit --session flag
            //   2. payload already has sessionId (from --json)
            //   3. --cwd flag (server-side cwd → session match)
            //   4. env ROUX_SESSION_ID
            if let Some(s) = session {
                payload["sessionId"] = Value::String(s);
            }
            if payload.get("sessionId").is_none() {
                if let Some(sid) = get_session_id() {
                    payload["sessionId"] = Value::String(sid);
                }
            }

            let mut args = serde_json::Map::new();
            args.insert("payload".into(), payload);
            if let Some(c) = cwd {
                args.insert("cwd".into(), Value::String(c));
            }

            run_socket_command(serde_json::json!({
                "command": "notify",
                "args": Value::Object(args),
            }));
        }

        Commands::Document { action } => handle_document(action),
        Commands::Notes { action } => handle_notes(action),
    }
}

fn scope_name(a: &NotesAction) -> Option<&'static str> {
    match a {
        NotesAction::Global { .. } => Some("global"),
        NotesAction::Project { .. } => Some("project"),
        NotesAction::Repo { .. } => Some("repo"),
        NotesAction::Session { .. } => Some("session"),
        NotesAction::Search { .. } | NotesAction::Root => None,
    }
}

fn build_target(scope: &str, topic: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "scope": scope,
        "sessionId": get_session_id(),
        "topic": topic,
        "overrideSlug": serde_json::Value::Null,
    })
}

fn read_stdin_if_needed(content: Option<String>) -> String {
    match content {
        Some(s) => s,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            let _ = std::io::stdin().read_to_string(&mut buf);
            // Trim one trailing newline — echo's default behavior shouldn't
            // double-space-out every appended entry.
            if buf.ends_with('\n') {
                buf.pop();
            }
            buf
        }
    }
}

fn handle_notes(action: NotesAction) {
    match action {
        NotesAction::Root => {
            run_socket_command(serde_json::json!({
                "command": "notes-vault-root",
                "args": {},
            }));
        }
        NotesAction::Search { tags, scope, tag_exact } => {
            run_socket_command(serde_json::json!({
                "command": "notes-search",
                "args": {
                    "tags": tags,
                    "scope": scope,
                    "exact": tag_exact,
                },
            }));
        }
        _ => {
            let scope = scope_name(&action).expect("scope-aware variants only");
            match action {
                NotesAction::Global { action }
                | NotesAction::Project { action }
                | NotesAction::Repo { action }
                | NotesAction::Session { action } => handle_notes_verb(scope, action),
                _ => unreachable!(),
            }
        }
    }
}

fn handle_notes_verb(scope: &str, verb: NotesScopeVerb) {
    match verb {
        NotesScopeVerb::Show { topic } => {
            run_socket_command(serde_json::json!({
                "command": "notes-read",
                "args": build_target(scope, topic),
            }));
        }
        NotesScopeVerb::Append { topic, content, timestamp, tags } => {
            let body = read_stdin_if_needed(content);
            run_socket_command(serde_json::json!({
                "command": "notes-append",
                "args": {
                    "target": build_target(scope, topic),
                    "content": body,
                    "timestamped": timestamp,
                    "tags": tags,
                },
            }));
        }
        NotesScopeVerb::Write { topic, content, tags } => {
            let body = read_stdin_if_needed(content);
            run_socket_command(serde_json::json!({
                "command": "notes-write",
                "args": {
                    "target": build_target(scope, topic),
                    "content": body,
                    "tags": tags,
                },
            }));
        }
        NotesScopeVerb::Path { topic, dir } => {
            run_socket_command(serde_json::json!({
                "command": "notes-path",
                "args": {
                    "target": build_target(scope, topic),
                    "dir": dir,
                },
            }));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── resolve_path ────────────────────────────────────────

    #[test]
    fn resolve_path_keeps_absolute_path_as_is() {
        // "/" always exists on Unix, canonicalize succeeds.
        let got = resolve_path("/");
        assert!(got.starts_with('/'));
    }

    #[test]
    fn resolve_path_turns_relative_into_absolute() {
        let got = resolve_path(".");
        assert!(std::path::Path::new(&got).is_absolute(), "got {}", got);
    }

    #[test]
    fn resolve_path_nonexistent_falls_back_to_uncanonicalized_absolute() {
        // canonicalize() fails on missing paths; resolve_path must still
        // return an absolute string rather than panicking or returning "".
        let got = resolve_path("/definitely/not/a/real/path/roux-test");
        assert!(std::path::Path::new(&got).is_absolute(), "got {}", got);
        assert!(got.contains("roux-test"));
    }

    // ── resolve_target ──────────────────────────────────────

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn resolve_target_no_flags_uses_env() {
        // The vanilla in-session case: caller has env vars, no flags.
        let (sid, pid) = resolve_target(None, None, s("env-sid"), s("env-pid"));
        assert_eq!(sid.as_deref(), Some("env-sid"));
        assert_eq!(pid.as_deref(), Some("env-pid"));
    }

    #[test]
    fn resolve_target_explicit_session_drops_env_pane_for_other_session() {
        // Issue #127: caller in session A targets session B with --session B.
        // The env pane belongs to A and must NOT leak into the request — that
        // is exactly what was routing writes back to the calling session's
        // (nonexistent) PTY.
        let (sid, pid) = resolve_target(s("other-sid"), None, s("env-sid"), s("env-pid"));
        assert_eq!(sid.as_deref(), Some("other-sid"));
        assert!(
            pid.is_none(),
            "env pane must be dropped when --session targets a different session"
        );
    }

    #[test]
    fn resolve_target_explicit_session_matching_env_keeps_env_pane() {
        // If the caller redundantly passes --session matching their own env,
        // the env pane is still applicable and should pass through.
        let (sid, pid) = resolve_target(s("env-sid"), None, s("env-sid"), s("env-pid"));
        assert_eq!(sid.as_deref(), Some("env-sid"));
        assert_eq!(pid.as_deref(), Some("env-pid"));
    }

    #[test]
    fn resolve_target_explicit_session_and_pane_pass_through() {
        let (sid, pid) = resolve_target(s("flag-sid"), s("flag-pid"), s("env-sid"), s("env-pid"));
        assert_eq!(sid.as_deref(), Some("flag-sid"));
        assert_eq!(pid.as_deref(), Some("flag-pid"));
    }

    #[test]
    fn resolve_target_explicit_pane_only_uses_env_session() {
        // Backwards-compatible behavior: --pane alone keeps inheriting the
        // env session.
        let (sid, pid) = resolve_target(None, s("flag-pid"), s("env-sid"), s("env-pid"));
        assert_eq!(sid.as_deref(), Some("env-sid"));
        assert_eq!(pid.as_deref(), Some("flag-pid"));
    }

    #[test]
    fn resolve_target_no_env_no_flags_yields_none() {
        let (sid, pid) = resolve_target(None, None, None, None);
        assert!(sid.is_none());
        assert!(pid.is_none());
    }

    // ── Cli parsing ─────────────────────────────────────────

    #[test]
    fn transcript_summary_extracts_last_user_prompt_and_assistant_response() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"user","message":{"content":"first prompt"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"first response"}]}}
{"type":"user","message":{"content":[{"type":"tool_result","content":"ignored"}]}}
{"type":"user","message":{"content":[{"type":"text","text":"final prompt"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"final response"}]}}
"#,
        )
        .unwrap();

        let (query, response) = extract_transcript_summary(path.to_str().unwrap()).unwrap();
        assert_eq!(query.as_deref(), Some("final prompt"));
        assert_eq!(response.as_deref(), Some("final response"));
    }

    #[test]
    fn transcript_summary_truncates_long_text() {
        let long = "x".repeat(250);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        std::fs::write(
            &path,
            format!(
                r#"{{"type":"user","message":{{"content":{long:?}}}}}
"#
            ),
        )
        .unwrap();

        let (query, _) = extract_transcript_summary(path.to_str().unwrap()).unwrap();
        let query = query.unwrap();
        assert_eq!(query.chars().count(), 200);
        assert!(query.ends_with("..."));
    }

    #[test]
    fn cli_parses_app_with_default_path() {
        let cli = Cli::try_parse_from(["roux", "app"]).unwrap();
        match cli.command {
            Commands::App { path } => assert_eq!(path, "."),
            other => panic!("expected App, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn cli_parses_app_with_explicit_path() {
        let cli = Cli::try_parse_from(["roux", "app", "/tmp/foo"]).unwrap();
        match cli.command {
            Commands::App { path } => assert_eq!(path, "/tmp/foo"),
            _ => panic!("expected App"),
        }
    }

    #[test]
    fn cli_parses_session_list() {
        let cli = Cli::try_parse_from(["roux", "session", "list"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::List } => {}
            _ => panic!("expected Session::List"),
        }
    }

    #[test]
    fn cli_parses_work_item_create() {
        let cli = Cli::try_parse_from([
            "roux",
            "work-item",
            "create",
            "Fix login",
            "--body",
            "Add regression coverage",
            "--status",
            "todo",
            "--project",
            "proj-1",
            "--sort-order",
            "42.5",
        ])
        .unwrap();
        match cli.command {
            Commands::WorkItem {
                action:
                    WorkItemAction::Create(WorkItemCreateArgs {
                        title,
                        body,
                        status,
                        project,
                        sort_order,
                        ..
                    }),
            } => {
                assert_eq!(title, "Fix login");
                assert_eq!(body.as_deref(), Some("Add regression coverage"));
                assert_eq!(status.as_deref(), Some("todo"));
                assert_eq!(project.as_deref(), Some("proj-1"));
                assert_eq!(sort_order, Some(42.5));
            }
            _ => panic!("expected WorkItem::Create"),
        }
    }

    #[test]
    fn work_item_update_can_clear_branch_and_disable_fetch_first() {
        let cli = Cli::try_parse_from([
            "roux",
            "work-item",
            "update",
            "wi-1",
            "--title",
            "Fix login",
            "--branch",
            "--fetch-first=false",
        ])
        .unwrap();
        match cli.command {
            Commands::WorkItem {
                action: WorkItemAction::Update(WorkItemUpdateArgs { branch, fetch_first, .. }),
            } => {
                assert_eq!(branch, Some(None));
                assert_eq!(fetch_first, Some(false));
            }
            _ => panic!("expected WorkItem::Update"),
        }
    }

    #[test]
    fn cli_parses_kanban_alias_start() {
        let cli = Cli::try_parse_from([
            "roux",
            "kanban",
            "start",
            "wi-1",
            "--profile",
            "claude",
            "--repo-path",
            "/repo",
            "--fetch-first",
        ])
        .unwrap();
        match cli.command {
            Commands::WorkItem {
                action:
                    WorkItemAction::Start(WorkItemStartArgs {
                        id, profile, repo_path, fetch_first, ..
                    }),
            } => {
                assert_eq!(id, "wi-1");
                assert_eq!(profile.as_deref(), Some("claude"));
                assert_eq!(repo_path.as_deref(), Some("/repo"));
                assert!(fetch_first);
            }
            _ => panic!("expected WorkItem::Start"),
        }
    }

    #[test]
    fn work_item_start_uses_start_socket_command() {
        let request = build_work_item_start_request(WorkItemStartArgs {
            id: "wi-1".into(),
            profile: Some("claude".into()),
            repo_path: Some("/repo".into()),
            name: Some("Fix login".into()),
            worktree_path: None,
            branch: Some("feat/login".into()),
            base: Some("origin/main".into()),
            fetch_first: true,
            force_start: true,
            fix_ci: true,
        });

        assert_eq!(request["command"], "work-item-start");
        assert_eq!(request["args"]["id"], "wi-1");
        assert_eq!(request["args"]["profile"], "claude");
        assert_eq!(request["args"]["repoPath"], "/repo");
        assert_eq!(request["args"]["name"], "Fix login");
        assert_eq!(request["args"]["branch"], "feat/login");
        assert_eq!(request["args"]["base"], "origin/main");
        assert_eq!(request["args"]["fetchFirst"], true);
        assert_eq!(request["args"]["forceStart"], true);
        assert_eq!(request["args"]["fixCi"], true);
    }

    #[test]
    fn work_item_start_fix_ci_requires_daemon_capability() {
        let params = WorkItemStartArgs {
            id: "wi-1".into(),
            profile: None,
            repo_path: None,
            name: None,
            worktree_path: None,
            branch: None,
            base: None,
            fetch_first: false,
            force_start: false,
            fix_ci: true,
        };
        let status = serde_json::json!({
            "capabilities": ["work-item-start"],
        });

        let err = validate_work_item_start_daemon_capabilities(&params, &status).unwrap_err();

        assert!(err.contains("work-item-start-fix-ci"));
    }

    #[test]
    fn work_item_start_fix_ci_accepts_daemon_capability() {
        let params = WorkItemStartArgs {
            id: "wi-1".into(),
            profile: None,
            repo_path: None,
            name: None,
            worktree_path: None,
            branch: None,
            base: None,
            fetch_first: false,
            force_start: false,
            fix_ci: true,
        };
        let status = serde_json::json!({
            "capabilities": ["work-item-start", "work-item-start-fix-ci"],
        });

        assert!(validate_work_item_start_daemon_capabilities(&params, &status).is_ok());
    }

    #[test]
    fn work_item_start_resolves_cli_paths_before_socket_request() {
        let request = build_work_item_start_request(WorkItemStartArgs {
            id: "wi-1".into(),
            profile: None,
            repo_path: Some(".".into()),
            name: None,
            worktree_path: Some("./wt".into()),
            branch: None,
            base: None,
            fetch_first: false,
            force_start: false,
            fix_ci: false,
        });

        assert_eq!(request["args"]["repoPath"], resolve_path("."));
        assert_eq!(request["args"]["worktreePath"], resolve_path("./wt"));
    }

    #[test]
    fn work_item_plan_uses_plan_socket_command_and_resolves_paths() {
        let request = build_work_item_plan_request(WorkItemPlanArgs {
            id: "wi-1".into(),
            profile: Some("claude".into()),
            repo_path: Some(".".into()),
            name: Some("Plan login".into()),
            worktree_path: Some("./wt".into()),
            replace_active: true,
        });

        assert_eq!(request["command"], "work-item-plan");
        assert_eq!(request["args"]["id"], "wi-1");
        assert_eq!(request["args"]["profile"], "claude");
        assert_eq!(request["args"]["repoPath"], resolve_path("."));
        assert_eq!(request["args"]["name"], "Plan login");
        assert_eq!(request["args"]["worktreePath"], resolve_path("./wt"));
        assert_eq!(request["args"]["replaceActive"], true);
    }

    #[test]
    fn cli_parses_work_item_accept() {
        let cli = Cli::try_parse_from(["roux", "work-item", "accept", "wi-1"]).unwrap();
        match cli.command {
            Commands::WorkItem { action: WorkItemAction::Accept { id } } => {
                assert_eq!(id, "wi-1");
            }
            _ => panic!("expected WorkItem::Accept"),
        }
    }

    #[test]
    fn cli_parses_work_item_review_request() {
        let cli = Cli::try_parse_from([
            "roux",
            "work-item",
            "review",
            "request",
            "run-1",
            "--summary",
            "Implemented review package",
            "--test",
            "npm run test",
            "--changed-file",
            "src/lib/workItems/reviewPackage.ts",
        ])
        .unwrap();
        match cli.command {
            Commands::WorkItem {
                action:
                    WorkItemAction::Review {
                        action:
                            WorkItemReviewAction::Request { run_id, summary, tests, changed_files },
                    },
            } => {
                assert_eq!(run_id, "run-1");
                assert_eq!(summary.as_deref(), Some("Implemented review package"));
                assert_eq!(tests, ["npm run test"]);
                assert_eq!(changed_files, ["src/lib/workItems/reviewPackage.ts"]);
            }
            _ => panic!("expected WorkItem::Review::Request"),
        }
    }

    #[test]
    fn work_item_review_request_uses_socket_command() {
        let request = build_work_item_review_request(WorkItemReviewRequestArgs {
            run_id: "run-1".into(),
            summary: Some("Implemented review package".into()),
            tests: vec!["npm run test".into()],
            changed_files: vec!["src/lib/workItems/reviewPackage.ts".into()],
        });

        assert_eq!(request["command"], "work-item-review-request");
        assert_eq!(request["args"]["runId"], "run-1");
        assert_eq!(request["args"]["summary"], "Implemented review package");
        assert_eq!(request["args"]["tests"][0], "npm run test");
        assert_eq!(request["args"]["changedFiles"][0], "src/lib/workItems/reviewPackage.ts");
    }

    #[test]
    fn cli_parses_work_item_review_request_changes() {
        let cli = Cli::try_parse_from([
            "roux",
            "work-item",
            "review",
            "request-changes",
            "run-1",
            "--note",
            "Add coverage",
            "--status",
            "planning",
        ])
        .unwrap();
        match cli.command {
            Commands::WorkItem {
                action:
                    WorkItemAction::Review {
                        action: WorkItemReviewAction::RequestChanges { target, note, status },
                    },
            } => {
                assert_eq!(target, "run-1");
                assert_eq!(note, "Add coverage");
                assert_eq!(status.as_deref(), Some("planning"));
            }
            _ => panic!("expected WorkItem::Review::RequestChanges"),
        }
    }

    #[test]
    fn work_item_review_request_changes_uses_socket_command() {
        let request = build_work_item_review_request_changes(
            "run-1".into(),
            "Add coverage".into(),
            Some("planning".into()),
        );

        assert_eq!(request["command"], "work-item-review-request-changes");
        assert_eq!(request["args"]["id"], "run-1");
        assert_eq!(request["args"]["note"], "Add coverage");
        assert_eq!(request["args"]["status"], "planning");
    }

    #[test]
    fn work_item_decision_options_parse_value_label_pairs() {
        let options =
            parse_work_item_decision_options(vec!["ship=Ship it".into(), "hold".into()]).unwrap();

        assert_eq!(options[0]["value"], "ship");
        assert_eq!(options[0]["label"], "Ship it");
        assert_eq!(options[1]["value"], "hold");
        assert_eq!(options[1]["label"], "hold");
    }

    #[test]
    fn work_item_decision_create_timeout_requires_default_value() {
        let err = build_work_item_decision_create_request(WorkItemDecisionCreateArgs {
            run_id: "run-1".into(),
            question: "Ship?".into(),
            options: vec!["yes=Yes".into()],
            default_value: None,
            timeout_at: None,
            timeout_seconds: Some(60),
            timeout_ms: None,
        })
        .expect_err("timeout without default should fail");

        assert!(err.contains("--default-value"));
    }

    #[test]
    fn work_item_import_requires_one_source() {
        assert!(build_work_item_import_request(None, None).is_err());
        assert!(build_work_item_import_request(Some("/tmp/items.json".into()), Some("[]".into()))
            .is_err());

        let request = build_work_item_import_request(None, Some(r#"[{"title":"A"}]"#.into()))
            .expect("inline items json should parse");
        assert_eq!(request["command"], "work-item-import");
        assert_eq!(request["args"]["items"][0]["title"], "A");
    }

    #[test]
    fn work_item_import_resolves_file_path_before_socket_request() {
        let request = build_work_item_import_request(Some("items.json".into()), None)
            .expect("file path import should build");

        assert_eq!(request["command"], "work-item-import");
        assert_eq!(request["args"]["path"], resolve_path("items.json"));
    }

    #[test]
    fn cli_parses_document_attach_text() {
        let cli = Cli::try_parse_from([
            "roux",
            "document",
            "attach",
            "--session",
            "sess-1",
            "--title",
            "Plan",
            "--text",
            "Use the plan.",
        ])
        .unwrap();
        match cli.command {
            Commands::Document {
                action:
                    DocumentAction::Attach(DocumentAttachArgs {
                        session,
                        work_item,
                        title,
                        text,
                        file,
                        ..
                    }),
            } => {
                assert_eq!(session.as_deref(), Some("sess-1"));
                assert_eq!(work_item, None);
                assert_eq!(title.as_deref(), Some("Plan"));
                assert_eq!(text.as_deref(), Some("Use the plan."));
                assert_eq!(file, None);
            }
            _ => panic!("expected Document::Attach"),
        }
    }

    #[test]
    fn document_attach_text_uses_socket_command() {
        let request = build_document_attach_request(DocumentAttachArgs {
            session: Some("sess-1".into()),
            work_item: None,
            title: Some("Plan".into()),
            text: Some("Use the plan.".into()),
            file: None,
            mime_type: Some("text/markdown".into()),
        })
        .expect("text document request should build");

        assert_eq!(request["command"], "document-attach");
        assert_eq!(request["args"]["targetKind"], "session");
        assert_eq!(request["args"]["targetId"], "sess-1");
        assert_eq!(request["args"]["title"], "Plan");
        assert_eq!(request["args"]["contentKind"], "text");
        assert_eq!(request["args"]["content"], "Use the plan.");
        assert_eq!(request["args"]["mimeType"], "text/markdown");
    }

    #[test]
    fn document_get_uses_socket_command() {
        let request = build_document_get_request("sess-1.att-1".to_string());

        assert_eq!(request["command"], "document-get");
        assert_eq!(request["args"]["id"], "sess-1.att-1");
    }

    #[test]
    fn cli_parses_hook_status_compat_command() {
        let cli = Cli::try_parse_from(["roux", "hook", "working"]).unwrap();
        match cli.command {
            Commands::Hook { action: HookAction::Working } => {}
            _ => panic!("expected Hook::Working"),
        }
    }

    #[test]
    fn cli_parses_hook_show_with_repo_path() {
        let cli = Cli::try_parse_from(["roux", "hook", "show", "--repo-path", "/repo"]).unwrap();
        match cli.command {
            Commands::Hook { action: HookAction::Show(HookShowArgs { repo_path }) } => {
                assert_eq!(repo_path.as_deref(), Some("/repo"));
            }
            _ => panic!("expected Hook::Show"),
        }
    }

    #[test]
    fn cli_parses_hook_run_with_context_and_extra_args() {
        let cli = Cli::try_parse_from([
            "roux",
            "hook",
            "run",
            "post-watch-success",
            "--repo-path",
            "/repo",
            "--worktree-path",
            "/repo/.worktrees/x",
            "--branch",
            "feat/x",
            "--session",
            "sid",
            "--task",
            "tid",
            "--scope",
            "session",
            "--provider",
            "worktrunk",
            "--",
            "--verbose",
        ])
        .unwrap();
        match cli.command {
            Commands::Hook { action: HookAction::Run(args) } => {
                let HookRunArgs {
                    event,
                    repo_path,
                    worktree_path,
                    branch,
                    session,
                    task,
                    scope,
                    provider,
                    extra,
                    ..
                } = *args;
                assert_eq!(event, "post-watch-success");
                assert_eq!(repo_path.as_deref(), Some("/repo"));
                assert_eq!(worktree_path.as_deref(), Some("/repo/.worktrees/x"));
                assert_eq!(branch.as_deref(), Some("feat/x"));
                assert_eq!(session.as_deref(), Some("sid"));
                assert_eq!(task.as_deref(), Some("tid"));
                assert_eq!(scope.as_deref(), Some("session"));
                assert_eq!(provider.as_deref(), Some("worktrunk"));
                assert_eq!(extra, vec!["--verbose"]);
            }
            _ => panic!("expected Hook::Run"),
        }
    }

    #[test]
    fn cli_parses_session_poll_with_session_id() {
        let cli = Cli::try_parse_from(["roux", "session", "poll", "-s", "sid-1"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Poll { session } } => {
                assert_eq!(session.as_deref(), Some("sid-1"));
            }
            _ => panic!("expected Session::Poll"),
        }
    }

    #[test]
    fn cli_parses_session_send_default_has_enter_true() {
        let cli = Cli::try_parse_from(["roux", "session", "send", "hello"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { text, no_enter, .. } } => {
                assert_eq!(text, "hello");
                assert!(!no_enter, "--no-enter not passed, so no_enter must be false");
            }
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_send_with_no_enter_flag() {
        let cli = Cli::try_parse_from(["roux", "session", "send", "hello", "--no-enter"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { no_enter, .. } } => assert!(no_enter),
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_send_with_session_and_pane() {
        let cli = Cli::try_parse_from(["roux", "session", "send", "hi", "-s", "sid", "-p", "pid"])
            .unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { session, pane, .. } } => {
                assert_eq!(session.as_deref(), Some("sid"));
                assert_eq!(pane.as_deref(), Some("pid"));
            }
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_create_with_full_options() {
        let cli = Cli::try_parse_from([
            "roux",
            "session",
            "create",
            "--name",
            "feat-x",
            "--worktree-branch",
            "feat/x",
            "--profile",
            "claude",
            "-f",
            "--debug",
            "-f",
            "--model=opus",
        ])
        .unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Create {
                        name,
                        worktree_branch,
                        from,
                        profile,
                        flags,
                        working_dir,
                        prompt,
                    },
            } => {
                assert_eq!(name.as_deref(), Some("feat-x"));
                assert_eq!(worktree_branch.as_deref(), Some("feat/x"));
                assert!(from.is_none());
                assert_eq!(profile.as_deref(), Some("claude"));
                assert_eq!(flags, vec!["--debug", "--model=opus"]);
                assert!(working_dir.is_none());
                assert!(prompt.is_none());
            }
            _ => panic!("expected Session::Create"),
        }
    }

    #[test]
    fn cli_parses_session_kill() {
        let cli = Cli::try_parse_from(["roux", "session", "kill", "--session", "sid-1"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Kill { session } } => {
                assert_eq!(session.as_deref(), Some("sid-1"));
            }
            _ => panic!("expected Session::Kill"),
        }
    }

    #[test]
    fn cli_parses_session_send_with_pane_type() {
        let cli = Cli::try_parse_from([
            "roux",
            "session",
            "send",
            "ls",
            "--session",
            "sid-1",
            "--pane-type",
            "shell",
        ])
        .unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { text, pane_type, pane, .. } } => {
                assert_eq!(text, "ls");
                assert_eq!(pane_type.as_deref(), Some("shell"));
                assert!(pane.is_none());
            }
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_create_with_prompt() {
        let cli = Cli::try_parse_from([
            "roux",
            "session",
            "create",
            "--working-dir",
            "/tmp/repo",
            "--prompt",
            "fix the bug",
        ])
        .unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Create { working_dir, prompt, .. } } => {
                assert_eq!(working_dir.as_deref(), Some("/tmp/repo"));
                assert_eq!(prompt.as_deref(), Some("fix the bug"));
            }
            _ => panic!("expected Session::Create"),
        }
    }

    #[test]
    fn cli_parses_session_rename() {
        let cli = Cli::try_parse_from([
            "roux",
            "session",
            "rename",
            "--session",
            "sid-1",
            "--name",
            "new name",
        ])
        .unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Rename { name, session } } => {
                assert_eq!(name, "new name");
                assert_eq!(session.as_deref(), Some("sid-1"));
            }
            _ => panic!("expected Session::Rename"),
        }
    }

    #[test]
    fn cli_parses_session_create_with_from() {
        let cli = Cli::try_parse_from([
            "roux",
            "session",
            "create",
            "--working-dir",
            "/path/to/repo-b",
            "--worktree-branch",
            "feat/x",
            "--from",
            "origin/main",
        ])
        .unwrap();
        match cli.command {
            Commands::Session {
                action: SessionAction::Create { working_dir, worktree_branch, from, .. },
            } => {
                assert_eq!(working_dir.as_deref(), Some("/path/to/repo-b"));
                assert_eq!(worktree_branch.as_deref(), Some("feat/x"));
                assert_eq!(from.as_deref(), Some("origin/main"));
            }
            _ => panic!("expected Session::Create"),
        }
    }

    #[test]
    fn cli_parses_session_panes_list_with_session() {
        let cli = Cli::try_parse_from(["roux", "session", "panes", "list", "-s", "sid"]).unwrap();
        match cli.command {
            Commands::Session {
                action: SessionAction::Panes { action: PaneAction::List { session } },
            } => {
                assert_eq!(session.as_deref(), Some("sid"));
            }
            _ => panic!("expected Session::Panes::List"),
        }
    }

    #[test]
    fn cli_parses_session_panes_create_defaults() {
        let cli = Cli::try_parse_from(["roux", "session", "panes", "create", "-s", "sid"]).unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Panes {
                        action: PaneAction::Create { session, profile, direction, working_dir },
                    },
            } => {
                assert_eq!(session.as_deref(), Some("sid"));
                assert!(profile.is_none());
                assert_eq!(direction, "horizontal");
                assert!(working_dir.is_none());
            }
            _ => panic!("expected Session::Panes::Create"),
        }
    }

    #[test]
    fn cli_parses_session_panes_create_with_all_options() {
        let cli = Cli::try_parse_from([
            "roux", "session", "panes", "create", "-s", "sid", "-P", "shell", "-d", "vertical",
            "-w", "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Panes {
                        action: PaneAction::Create { profile, direction, working_dir, .. },
                    },
            } => {
                assert_eq!(profile.as_deref(), Some("shell"));
                assert_eq!(direction, "vertical");
                assert_eq!(working_dir.as_deref(), Some("/tmp"));
            }
            _ => panic!("expected Session::Panes::Create"),
        }
    }

    #[test]
    fn cli_rejects_removed_top_level_send() {
        // `roux send "x"` used to work; now it must live under `session`.
        let err = Cli::try_parse_from(["roux", "send", "x"]);
        assert!(err.is_err(), "top-level `send` must no longer parse");
    }

    #[test]
    fn cli_split_still_parses_with_direction() {
        let cli = Cli::try_parse_from(["roux", "split", "-d", "vertical"]).unwrap();
        match cli.command {
            Commands::Split { direction } => assert_eq!(direction, "vertical"),
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn cli_parses_attach_with_pty_id() {
        let cli =
            Cli::try_parse_from(["roux", "attach", "pty-1", "--max-bytes", "4096", "--no-input"])
                .unwrap();
        match cli.command {
            Commands::Attach { target, session, max_bytes, no_input } => {
                assert_eq!(target.as_deref(), Some("pty-1"));
                assert!(session.is_none());
                assert_eq!(max_bytes, 4096);
                assert!(no_input);
            }
            _ => panic!("expected Attach"),
        }
    }

    #[test]
    fn cli_parses_attach_with_session() {
        let cli = Cli::try_parse_from(["roux", "attach", "--session", "session-1"]).unwrap();
        match cli.command {
            Commands::Attach { target, session, max_bytes, no_input } => {
                assert!(target.is_none());
                assert_eq!(session.as_deref(), Some("session-1"));
                assert_eq!(max_bytes, 65536);
                assert!(!no_input);
            }
            _ => panic!("expected Attach"),
        }
    }

    #[test]
    fn cli_parses_mcp_command() {
        let cli = Cli::try_parse_from(["roux", "mcp"]).unwrap();
        match cli.command {
            Commands::Mcp => {}
            _ => panic!("expected Mcp"),
        }
    }

    #[test]
    fn cli_parses_install_claude_hooks_command() {
        let cli = Cli::try_parse_from(["roux", "install", "hooks", "--agent", "claude"]).unwrap();
        match cli.command {
            Commands::Install {
                action: InstallAction::Hooks(InstallHooksArgs { agent: InstallHooksAgent::Claude }),
            } => {}
            _ => panic!("expected Install::Hooks"),
        }
    }

    #[test]
    fn merge_roux_claude_hooks_replaces_roux_entries_and_preserves_user_hooks() {
        let cli_path = Path::new("/opt/roux/bin/roux");
        let mut settings = json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "AskUserQuestion",
                        "hooks": [
                            { "type": "command", "command": "/old/roux hook attention" }
                        ]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "echo keep-me" }
                        ]
                    }
                ]
            }
        });

        merge_roux_claude_hooks(&mut settings, cli_path).unwrap();

        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(entries.iter().any(|entry| {
            entry.get("matcher").and_then(|matcher| matcher.as_str()) == Some("AskUserQuestion")
                && entry.get("hooks").and_then(|hooks| hooks.as_array()).unwrap().iter().any(
                    |hook| {
                        hook.get("command").and_then(|command| command.as_str())
                            == Some("/opt/roux/bin/roux hook attention")
                    },
                )
        }));
        assert!(entries.iter().any(|entry| {
            entry.get("matcher").and_then(|matcher| matcher.as_str()) == Some("Bash")
        }));
        assert!(!entries.iter().any(|entry| {
            entry.get("hooks").and_then(|hooks| hooks.as_array()).unwrap().iter().any(|hook| {
                hook.get("command").and_then(|command| command.as_str())
                    == Some("/old/roux hook attention")
            })
        }));
    }

    #[test]
    fn daemon_timeout_from_env_uses_positive_milliseconds() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ROUX_TEST_TIMEOUT_MS", "2500");
        assert_eq!(
            daemon_timeout_from_env("ROUX_TEST_TIMEOUT_MS", Duration::from_secs(1)),
            Duration::from_millis(2500)
        );
        std::env::set_var("ROUX_TEST_TIMEOUT_MS", "0");
        assert_eq!(
            daemon_timeout_from_env("ROUX_TEST_TIMEOUT_MS", Duration::from_secs(1)),
            Duration::from_secs(1)
        );
        std::env::remove_var("ROUX_TEST_TIMEOUT_MS");
    }

    #[test]
    fn cli_parses_daemon_command() {
        let cli = Cli::try_parse_from(["roux", "daemon"]).unwrap();
        match cli.command {
            Commands::Daemon { action: None } => {}
            _ => panic!("expected Daemon"),
        }
    }

    #[test]
    fn cli_parses_daemon_status_command() {
        let cli = Cli::try_parse_from(["roux", "daemon", "status"]).unwrap();
        match cli.command {
            Commands::Daemon { action: Some(DaemonAction::Status) } => {}
            _ => panic!("expected Daemon::Status"),
        }
    }

    #[test]
    fn cli_parses_daemon_lifecycle_commands() {
        let start = Cli::try_parse_from(["roux", "daemon", "start"]).unwrap();
        assert!(matches!(start.command, Commands::Daemon { action: Some(DaemonAction::Start) }));

        let stop = Cli::try_parse_from(["roux", "daemon", "stop"]).unwrap();
        assert!(matches!(stop.command, Commands::Daemon { action: Some(DaemonAction::Stop) }));

        let restart = Cli::try_parse_from(["roux", "daemon", "restart"]).unwrap();
        assert!(matches!(
            restart.command,
            Commands::Daemon { action: Some(DaemonAction::Restart) }
        ));

        let clear = Cli::try_parse_from(["roux", "daemon", "clear"]).unwrap();
        assert!(matches!(clear.command, Commands::Daemon { action: Some(DaemonAction::Clear) }));
    }

    #[test]
    fn cli_parses_daemon_logs_command() {
        let cli =
            Cli::try_parse_from(["roux", "daemon", "logs", "--lines", "25", "--follow"]).unwrap();
        match cli.command {
            Commands::Daemon { action: Some(DaemonAction::Logs { lines, follow }) } => {
                assert_eq!(lines, 25);
                assert!(follow);
            }
            _ => panic!("expected Daemon::Logs"),
        }
    }

    #[test]
    fn cli_parses_daemon_connect_command() {
        let cli = Cli::try_parse_from([
            "roux",
            "daemon",
            "connect",
            "tcp://100.73.57.24:7777",
            "--auth-token",
            "secret-token",
        ])
        .unwrap();

        match cli.command {
            Commands::Daemon {
                action: Some(DaemonAction::Connect { socket, auth_token: Some(auth_token) }),
            } => {
                assert_eq!(socket, "tcp://100.73.57.24:7777");
                assert_eq!(auth_token, "secret-token");
            }
            _ => panic!("expected Daemon::Connect"),
        }
    }

    #[test]
    fn daemon_connect_warns_when_tcp_endpoint_has_no_auth_token_source() {
        let endpoint = platform::SocketEndpoint::Tcp("127.0.0.1:7777".to_string());

        assert_eq!(
            daemon_connect_auth_warning(&endpoint, false, false),
            Some(TCP_CONNECT_AUTH_WARNING)
        );
    }

    #[test]
    fn daemon_connect_does_not_warn_when_tcp_auth_token_source_exists() {
        let endpoint = platform::SocketEndpoint::Tcp("127.0.0.1:7777".to_string());

        assert_eq!(daemon_connect_auth_warning(&endpoint, true, false), None);
        assert_eq!(daemon_connect_auth_warning(&endpoint, false, true), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn daemon_connect_does_not_warn_for_unix_endpoint_without_auth_token() {
        let endpoint = platform::SocketEndpoint::Unix("/tmp/roux.sock".into());

        assert_eq!(daemon_connect_auth_warning(&endpoint, false, false), None);
    }

    #[test]
    fn cli_parses_daemon_disconnect_command() {
        let cli = Cli::try_parse_from(["roux", "daemon", "disconnect"]).unwrap();

        assert!(matches!(cli.command, Commands::Daemon { action: Some(DaemonAction::Disconnect) }));
    }

    #[cfg(unix)]
    #[test]
    fn private_config_file_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roux-socket-token");

        write_private_config_file(&path, "secret-token").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "secret-token");
    }

    #[cfg(unix)]
    #[test]
    fn private_config_file_replaces_permissive_existing_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roux-socket-token");
        std::fs::write(&path, "old-token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_private_config_file(&path, "new-token").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new-token");
    }

    #[test]
    fn cli_parses_daemon_run_command() {
        let cli = Cli::try_parse_from(["roux", "daemon", "run", "printf hi", "--working-dir", "."])
            .unwrap();
        match cli.command {
            Commands::Daemon {
                action: Some(DaemonAction::Run { command, working_dir: Some(working_dir) }),
            } => {
                assert_eq!(command, "printf hi");
                assert_eq!(working_dir, ".");
            }
            _ => panic!("expected Daemon::Run"),
        }
    }

    #[test]
    fn cli_parses_daemon_output_command() {
        let cli = Cli::try_parse_from([
            "roux",
            "daemon",
            "output",
            "daemon-process-1",
            "--max-bytes",
            "42",
        ])
        .unwrap();
        match cli.command {
            Commands::Daemon { action: Some(DaemonAction::Output { id, max_bytes }) } => {
                assert_eq!(id, "daemon-process-1");
                assert_eq!(max_bytes, 42);
            }
            _ => panic!("expected Daemon::Output"),
        }
    }

    #[test]
    fn cli_parses_daemon_pty_run_command() {
        let cli = Cli::try_parse_from([
            "roux",
            "daemon",
            "pty-run",
            "printf hi",
            "--working-dir",
            ".",
            "--session-id",
            "session-a",
            "--pane-id",
            "pane-a",
            "--profile",
            "task",
            "--cols",
            "120",
            "--rows",
            "40",
        ])
        .unwrap();
        match cli.command {
            Commands::Daemon {
                action:
                    Some(DaemonAction::PtyRun {
                        command,
                        working_dir: Some(working_dir),
                        session_id: Some(session_id),
                        pane_id: Some(pane_id),
                        profile: Some(profile),
                        cols: Some(cols),
                        rows: Some(rows),
                    }),
            } => {
                assert_eq!(command, "printf hi");
                assert_eq!(working_dir, ".");
                assert_eq!(session_id, "session-a");
                assert_eq!(pane_id, "pane-a");
                assert_eq!(profile, "task");
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("expected Daemon::PtyRun"),
        }
    }

    #[test]
    fn cli_parses_daemon_pty_control_commands() {
        let output = Cli::try_parse_from([
            "roux",
            "daemon",
            "pty-output",
            "daemon-pty-1",
            "--max-bytes",
            "42",
        ])
        .unwrap();
        match output.command {
            Commands::Daemon { action: Some(DaemonAction::PtyOutput { id, max_bytes }) } => {
                assert_eq!(id, "daemon-pty-1");
                assert_eq!(max_bytes, 42);
            }
            _ => panic!("expected Daemon::PtyOutput"),
        }

        let list = Cli::try_parse_from(["roux", "daemon", "ptys"]).unwrap();
        assert!(matches!(list.command, Commands::Daemon { action: Some(DaemonAction::Ptys) }));

        let write = Cli::try_parse_from(["roux", "daemon", "pty-write", "daemon-pty-1", "hello\n"])
            .unwrap();
        assert!(matches!(
            write.command,
            Commands::Daemon { action: Some(DaemonAction::PtyWrite { .. }) }
        ));

        let resize =
            Cli::try_parse_from(["roux", "daemon", "pty-resize", "daemon-pty-1", "100", "30"])
                .unwrap();
        assert!(matches!(
            resize.command,
            Commands::Daemon { action: Some(DaemonAction::PtyResize { .. }) }
        ));

        let kill = Cli::try_parse_from(["roux", "daemon", "pty-kill", "daemon-pty-1"]).unwrap();
        assert!(matches!(
            kill.command,
            Commands::Daemon { action: Some(DaemonAction::PtyKill { .. }) }
        ));

        let watches = Cli::try_parse_from(["roux", "daemon", "watches"]).unwrap();
        assert!(matches!(
            watches.command,
            Commands::Daemon { action: Some(DaemonAction::Watches) }
        ));
    }

    #[test]
    fn cli_accepts_legacy_roux_cli_argv0() {
        let cli = Cli::try_parse_from(["roux-cli", "session", "list"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::List } => {}
            _ => panic!("expected Session::List"),
        }
    }
}
