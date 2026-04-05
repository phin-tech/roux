export type KeepOpen = "always" | "on-error" | "never";

export interface TaskDefinition {
  id: string;
  name: string;
  description: string;
  runner: string;
  command: string;
  keepOpen: KeepOpen;
}

export interface TaskGroup {
  runner: string;
  configFile: string;
  tasks: TaskDefinition[];
}

export interface TaskRun {
  taskId: string;
  ptyId: string;
  paneId: string | null;
  status: "running" | "succeeded" | "failed";
  exitCode: number | null;
  outputLines: string[];
  startedAt: number;
}
