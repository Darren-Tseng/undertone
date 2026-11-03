import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DictationState =
  | "idle"
  | "recording"
  | "transcribing"
  | "done"
  | "error"
  | "busy";

export interface Settings {
  hotkey: string;
  model: string;
  language: string;
  device: string;
  computeType: string;
  autoPaste: boolean;
  copyToClipboard: boolean;
  maxRecordSecs: number;
  inputDevice: string | null;
  workspaceDir: string;
  sidecarPort: number;
  initialPrompt: string;
  pythonPath: string;
}

export interface TranscriptEntry {
  id: string;
  path: string;
  text: string;
  created: string;
  duration: number;
  model: string;
  language: string;
}

export type SidecarState = "stopped" | "starting" | "loading" | "ready" | "error";

export interface LastTranscription {
  at: string;
  model: string;
  device: string;
  computeType: string;
  language: string | null;
  audioSeconds: number;
  elapsedSeconds: number;
  realtimeFactor: number;
}

export interface EngineDetails {
  pid: number | null;
  python: string | null;
  fasterWhisper: string | null;
  cudaDevices: number;
  uptime: number | null;
  loadedModels: string[];
  loadingModels: string[];
  lastError: string | null;
  activeModel: string | null;
  device: string | null;
  computeType: string | null;
  totalTranscriptions: number;
  totalAudioSeconds: number;
  totalTranscribeSeconds: number;
  lastTranscription: LastTranscription | null;
}

export interface SidecarStatus {
  state: SidecarState;
  message: string;
  port: number;
  log: string;
  details: EngineDetails;
}

export interface ModelInfo {
  id: string;
  label: string;
  description: string;
  approxBytes: number;
  speed: number;
  accuracy: number;
  recommended: boolean;
  multilingual: boolean;
  downloaded: boolean;
  bytes: number;
  paths: string[];
  loaded: boolean;
  loading: boolean;
  active: boolean;
}

export interface ModelsOverview {
  models: ModelInfo[];
  cacheDir: string;
  totalBytes: number;
  downloadedCount: number;
}

export interface RuntimeState {
  recording: boolean;
  busy: boolean;
  elapsed: number;
  level: number;
  sidecar: SidecarStatus;
  hotkeyError: string | null;
}

export interface StatePayload {
  state: DictationState;
  message?: string | null;
}

export const api = {
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<Settings>("save_settings", { settings }),
  runtime: () => invoke<RuntimeState>("get_runtime_state"),
  start: () => invoke<void>("start_dictation"),
  stop: () => invoke<void>("stop_dictation"),
  cancel: () => invoke<void>("cancel_dictation"),
  sidecarStatus: () => invoke<SidecarStatus>("sidecar_status"),
  restartSidecar: () => invoke<SidecarStatus>("restart_sidecar"),
  listModels: () => invoke<ModelsOverview>("list_models"),
  prepareModel: (model: string) =>
    invoke<SidecarStatus>("prepare_model", { model }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  openModelsDir: () => invoke<void>("open_models_dir"),
  devices: () => invoke<string[]>("list_input_devices"),
  transcripts: (query = "", limit = 200) =>
    invoke<TranscriptEntry[]>("list_transcripts", { query, limit }),
  deleteTranscript: (path: string) =>
    invoke<void>("delete_transcript", { path }),
  copy: (text: string) => invoke<void>("copy_text", { text }),
  reveal: (path: string) => invoke<void>("reveal_transcript", { path }),
  openWorkspace: () => invoke<void>("open_workspace"),
};

export const events = {
  state: (cb: (payload: StatePayload) => void): Promise<UnlistenFn> =>
    listen<StatePayload>("murmur://state", (event) => cb(event.payload)),
  transcript: (cb: (entry: TranscriptEntry) => void): Promise<UnlistenFn> =>
    listen<TranscriptEntry>("murmur://transcript", (event) =>
      cb(event.payload),
    ),
};
