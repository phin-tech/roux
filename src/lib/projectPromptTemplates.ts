import { get } from "svelte/store";
import type { Project, RouxSettings, Session, SessionBlueprint } from "$lib/types";
import type { SpawnProfile } from "$lib/panes/profiles";
import { renderProjectPromptTemplate as renderProjectPromptTemplateRaw } from "$lib/tauri";
import { getProjectById } from "$lib/stores/projects";
import { sessionState } from "$lib/stores/sessions";
import { settings } from "$lib/stores/settings";

export interface ProjectPromptTemplateSession {
  id: string;
  name: string;
  repo_root: string;
  worktree_path: string;
  worktree_name: string;
  branch: string | null;
  is_worktree: boolean;
  blueprint_id: string | null;
}

export interface ProjectPromptTemplateContext {
  project: {
    id: string | null;
    name: string;
    repo_roots: string[];
    context_paths: string[];
  };
  session: ProjectPromptTemplateSession;
  profile: {
    id: string | null;
    name: string | null;
    provider: string | null;
  };
  model: {
    name: string | null;
    family: string | null;
  };
  paths: {
    sessions_folder: string;
  };
  other_sessions: ProjectPromptTemplateSession[];
}

interface BuildProjectPromptContextOptions {
  project: Project;
  session: Session;
  profile: SpawnProfile | null;
  settings: RouxSettings;
  sessions: Session[];
}

interface ProjectPromptPreviewProject {
  id: string | null;
  name: string;
  repoRoots: string[];
  contextPaths: string[];
}

interface BuildProjectPromptPreviewContextOptions {
  project: ProjectPromptPreviewProject;
  blueprint: SessionBlueprint | null;
  profile: SpawnProfile | null;
  settings: RouxSettings;
  sessions: Session[];
}

function lastPathSegment(path: string): string {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function providerFamily(profile: SpawnProfile | null): string | null {
  return profile?.provider ?? null;
}

function modelName(settings: RouxSettings): string | null {
  const value = settings.defaultModel?.trim();
  return value ? value : null;
}

function sessionTemplateContext(session: Session): ProjectPromptTemplateSession {
  return {
    id: session.id,
    name: session.name,
    repo_root: session.repoRoot,
    worktree_path: session.worktreePath,
    worktree_name: lastPathSegment(session.worktreePath),
    branch: session.branch || null,
    is_worktree: session.isWorktree,
    blueprint_id: session.blueprintId ?? null,
  };
}

function sameProjectSessions(
  sessions: Session[],
  projectId: string | null | undefined,
  excludeSessionId: string,
): ProjectPromptTemplateSession[] {
  if (!projectId) return [];
  return sessions
    .filter((s) => s.projectId === projectId && s.id !== excludeSessionId && !s.archived)
    .map(sessionTemplateContext);
}

export function buildProjectPromptContext({
  project,
  session,
  profile,
  settings,
  sessions,
}: BuildProjectPromptContextOptions): ProjectPromptTemplateContext {
  const family = providerFamily(profile);
  return {
    project: {
      id: project.id,
      name: project.name,
      repo_roots: project.repoRoots ?? [],
      context_paths: project.contextPaths ?? [],
    },
    session: sessionTemplateContext(session),
    profile: {
      id: profile?.id ?? null,
      name: profile?.name ?? null,
      provider: family,
    },
    model: {
      name: modelName(settings),
      family,
    },
    paths: {
      sessions_folder: session.worktreePath,
    },
    other_sessions: sameProjectSessions(sessions, project.id, session.id),
  };
}

export function buildProjectPromptPreviewContext({
  project,
  blueprint,
  profile,
  settings,
  sessions,
}: BuildProjectPromptPreviewContextOptions): ProjectPromptTemplateContext {
  const repoRoot = blueprint?.repoRoot ?? project.repoRoots[0] ?? "";
  const worktreePath = blueprint?.worktreePath || repoRoot;
  const family = providerFamily(profile);
  const previewSession: ProjectPromptTemplateSession = {
    id: "preview",
    name: blueprint?.name || project.name || "Preview",
    repo_root: repoRoot,
    worktree_path: worktreePath,
    worktree_name: lastPathSegment(worktreePath),
    branch: blueprint?.branch ?? null,
    // Project blueprints map a branch-only target to SessionTarget::NewWorktree
    // when the session is created, so preview should expose worktree semantics.
    is_worktree: Boolean(blueprint?.branch || blueprint?.worktreePath),
    blueprint_id: blueprint?.id ?? null,
  };

  return {
    project: {
      id: project.id,
      name: project.name,
      repo_roots: project.repoRoots,
      context_paths: project.contextPaths,
    },
    session: previewSession,
    profile: {
      id: profile?.id ?? null,
      name: profile?.name ?? null,
      provider: family,
    },
    model: {
      name: modelName(settings),
      family,
    },
    paths: {
      sessions_folder: previewSession.worktree_path,
    },
    other_sessions: sameProjectSessions(sessions, project.id, previewSession.id),
  };
}

export async function renderProjectPromptTemplate(
  template: string,
  context: ProjectPromptTemplateContext,
): Promise<string> {
  if (!template.trim()) return "";
  return renderProjectPromptTemplateRaw(template, context);
}

export async function renderProjectPromptForSession(
  session: Session,
  profile: SpawnProfile | null,
  projectOverride?: Project | null,
): Promise<string> {
  const project = projectOverride ?? getProjectById(session.projectId);
  const template = project?.projectPrompt ?? "";
  if (!project || !template.trim()) return "";

  return renderProjectPromptTemplate(
    template,
    buildProjectPromptContext({
      project,
      session,
      profile,
      settings: get(settings),
      sessions: get(sessionState).sessions,
    }),
  );
}
