export type WatchScope =
  | { type: "global" }
  | { type: "session"; sessionId: string }
  | { type: "project"; projectId: string };

export type RuntimeState =
  | { type: "pending" }
  | { type: "active" }
  | { type: "paused" }
  | { type: "stopped" }
  | { type: "error"; message: string };

export type WatchKind =
  | { type: "githubAction"; repo: string; runId: number | null; workflow: string | null; branch: string | null }
  | { type: "httpHealth"; url: string; expectedStatus: number }
  | { type: "shellCommand"; command: string; workingDir: string | null; successExitCode: number }
  | { type: "task"; taskId: string; command: string; workingDir: string }
  | { type: "githubPr"; repo: string; prNumber: number };

export type WatchMode =
  | { type: "recurring"; intervalSecs: number }
  | { type: "oneShot" };

export type WatchOutcome = "success" | "failure" | "inProgress";

export interface GithubJob {
  name: string;
  status: string;
  conclusion: string | null;
  failedStep: string | null;
}

export interface PrReview {
  reviewer: string;
  state: string;
  url: string | null;
}

export interface PrCheckRun {
  name: string;
  conclusion: string | null;
  url: string | null;
}

export type WatchResult =
  | { type: "githubRun"; runId: number; status: string; conclusion: string | null; url: string; jobs: GithubJob[]; outcome: WatchOutcome }
  | { type: "httpCheck"; statusCode: number; responseTimeMs: number; outcome: WatchOutcome }
  | { type: "commandRun"; exitCode: number; stdout: string; stderr: string; outcome: WatchOutcome }
  | { type: "githubPr"; prNumber: number; state: string; title: string; url: string; headSha: string; draft: boolean; reviews: PrReview[]; checks: PrCheckRun[]; outcome: WatchOutcome };

export interface NotifyConfig {
  desktopNotification: boolean;
  onFailure: boolean;
  onSuccess: boolean;
}

export interface Watch {
  id: string;
  name: string;
  kind: WatchKind;
  mode: WatchMode;
  scope: WatchScope;
  runtimeState: RuntimeState;
  lastResult: WatchResult | null;
  lastChecked: number | null;
  notify: NotifyConfig;
  createdAt: number;
}

export interface WatchUpdateEvent {
  watch: Watch;
  changed: boolean;
  previousOutcome: WatchOutcome | null;
}

export interface CreateWatchConfig {
  name: string;
  kind: WatchKind;
  mode: WatchMode;
  scope: WatchScope;
  notify?: NotifyConfig;
}
