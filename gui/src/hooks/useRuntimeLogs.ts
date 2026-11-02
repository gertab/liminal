import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dispatch, SetStateAction, useCallback, useEffect, useRef, useState } from "react";
import {
  RuntimeContentFilter,
  RuntimeLogEntry,
  RuntimeLogStream,
  RuntimeState,
} from "../components/runtime-console/RuntimeConsole";
import {
  RuntimeEvent,
  RuntimeChannelCounters,
  RuntimeMessageActivity,
  RuntimeStageCounters,
  RuntimeStageState,
  RuntimeStageStates,
} from "../types";

type PipelineLogEvent = {
  stream: RuntimeLogStream;
  line: string;
  emitted_at_ms: number;
};

type PipelineStateEvent = {
  state: "idle" | "running" | "stopped" | "error";
  message: string | null;
  emitted_at_ms: number;
};

type PipelineRuntimeEventPayload = {
  event: RuntimeEvent;
  emitted_at_ms: number;
};

const maxRuntimeLogs = 500;
const maxRuntimeEvents = 1500;
const runtimeActivityTtlMs = 2200;

const emptyRuntimeMessageActivity: RuntimeMessageActivity = {
  stageIds: {},
  channelNames: {},
};

const defaultRuntimeContentFilter: RuntimeContentFilter = {
  logs: true,
  telemetry: false,
};

export function useRuntimeLogs({
  configPath,
  hasUnsavedDraft,
  onError,
}: {
  configPath: string;
  hasUnsavedDraft: boolean;
  onError: (message: string | null) => void;
}) {
  const [runtimeState, setRuntimeState] = useState<RuntimeState>("idle");
  const [runtimeLogs, setRuntimeLogs] = useState<RuntimeLogEntry[]>([]);
  const [runtimeEvents, setRuntimeEvents] = useState<RuntimeEvent[]>([]);
  const [runtimeStageStates, setRuntimeStageStates] = useState<RuntimeStageStates>({});
  const [runtimeStageCounters, setRuntimeStageCounters] = useState<RuntimeStageCounters>({});
  const [runtimeChannelCounters, setRuntimeChannelCounters] = useState<RuntimeChannelCounters>({});
  const [runtimeMessageActivity, setRuntimeMessageActivity] = useState<RuntimeMessageActivity>(
    emptyRuntimeMessageActivity,
  );
  const [runtimeLastMessageActivity, setRuntimeLastMessageActivity] =
    useState<RuntimeMessageActivity>(emptyRuntimeMessageActivity);
  const [runtimeLogFilter, setRuntimeLogFilter] = useState<"all" | "selection">("all");
  const [runtimeContentFilter, setRuntimeContentFilter] = useState<RuntimeContentFilter>(
    defaultRuntimeContentFilter,
  );
  const runtimeStateRef = useRef<RuntimeState>("idle");
  const clearedAtMsRef = useRef(0);
  const discardBackendLogsUntilNextRunRef = useRef(false);

  const setRuntimeStateTracked = useCallback((state: RuntimeState) => {
    runtimeStateRef.current = state;
    setRuntimeState(state);
  }, []);

  const appendRuntimeLog = useCallback(
    (stream: RuntimeLogStream, line: string, emittedAtMs?: number) => {
      if (emittedAtMs !== undefined && emittedAtMs <= clearedAtMsRef.current) {
        return;
      }

      const isBackendLog = emittedAtMs !== undefined;
      const timestampMs = emittedAtMs ?? Date.now();
      setRuntimeLogs((logs) => {
        const visibleLogs = logs.filter(
          (log) => (log.timestampMs ?? log.id) > clearedAtMsRef.current,
        );
        if (isBackendLog && discardBackendLogsUntilNextRunRef.current) {
          return visibleLogs;
        }
        if (isBackendLog && timestampMs <= clearedAtMsRef.current) {
          return visibleLogs;
        }

        return [
          ...visibleLogs,
          { id: timestampMs + Math.random(), stream, line, timestampMs },
        ].slice(-maxRuntimeLogs);
      });
    },
    [],
  );

  useEffect(() => {
    invoke<string>("pipeline_runtime_state")
      .then((state) => setRuntimeStateTracked(state === "running" ? "running" : "idle"))
      .catch(() => setRuntimeStateTracked("idle"));
  }, [setRuntimeStateTracked]);

  useEffect(() => {
    let disposed = false;
    const unlistenLog = listen<PipelineLogEvent>("pipeline://log", (event) => {
      if (!disposed && !discardBackendLogsUntilNextRunRef.current) {
        appendRuntimeLog(
          event.payload.stream,
          event.payload.line,
          event.payload.emitted_at_ms,
        );
      }
    });
    const unlistenState = listen<PipelineStateEvent>("pipeline://state", (event) => {
      if (disposed) {
        return;
      }

      setRuntimeStateTracked(event.payload.state === "running" ? "running" : "idle");
      if (event.payload.message && !discardBackendLogsUntilNextRunRef.current) {
        appendRuntimeLog("system", event.payload.message, event.payload.emitted_at_ms);
      }
    });
    const unlistenRuntimeEvent = listen<PipelineRuntimeEventPayload>(
      "pipeline://runtime-event",
      (event) => {
        if (!disposed) {
          if (
            !discardBackendLogsUntilNextRunRef.current &&
            event.payload.emitted_at_ms > clearedAtMsRef.current
          ) {
            setRuntimeEvents((events) => {
              const visibleEvents = events.filter(
                (runtimeEvent) => runtimeEvent.timestamp_ms > clearedAtMsRef.current,
              );
              if (
                discardBackendLogsUntilNextRunRef.current ||
                event.payload.emitted_at_ms <= clearedAtMsRef.current ||
                event.payload.event.timestamp_ms <= clearedAtMsRef.current
              ) {
                return visibleEvents;
              }

              return [...visibleEvents, event.payload.event].slice(-maxRuntimeEvents);
            });
          }
          applyRuntimeEvent(event.payload.event, setRuntimeStageStates);
          if (!discardBackendLogsUntilNextRunRef.current) {
            applyRuntimeMessageEvent(event.payload.event, setRuntimeMessageActivity);
            applyRuntimeLastMessageEvent(event.payload.event, setRuntimeLastMessageActivity);
            applyRuntimeCounters(
              event.payload.event,
              setRuntimeStageCounters,
              setRuntimeChannelCounters,
            );
          }
          applyPipelineRuntimeEvent(event.payload.event, setRuntimeStateTracked);
          if (
            !discardBackendLogsUntilNextRunRef.current &&
            shouldShowRuntimeEventInConsole(event.payload.event)
          ) {
            appendRuntimeLog(
              "system",
              formatRuntimeEvent(event.payload.event),
              event.payload.emitted_at_ms,
            );
          }
        }
      },
    );

    return () => {
      disposed = true;
      void unlistenLog.then((unlisten) => unlisten());
      void unlistenState.then((unlisten) => unlisten());
      void unlistenRuntimeEvent.then((unlisten) => unlisten());
    };
  }, [appendRuntimeLog]);

  useEffect(() => {
    const intervalId = window.setInterval(() => {
      setRuntimeMessageActivity((activity) => pruneRuntimeMessageActivity(activity, Date.now()));
    }, 500);

    return () => window.clearInterval(intervalId);
  }, []);

  const startRuntime = useCallback(async () => {
    if (hasUnsavedDraft) {
      onError("Save or revert the current draft before running the pipeline.");
      appendRuntimeLog("system", "Run blocked: save or revert the current draft first.");
      return;
    }

    discardBackendLogsUntilNextRunRef.current = false;
    setRuntimeStateTracked("starting");
    setRuntimeEvents([]);
    setRuntimeStageStates({});
    setRuntimeStageCounters({});
    setRuntimeChannelCounters({});
    setRuntimeMessageActivity(emptyRuntimeMessageActivity);
    setRuntimeLastMessageActivity(emptyRuntimeMessageActivity);
    onError(null);
    clearedAtMsRef.current = 0;
    setRuntimeLogs([]);
    appendRuntimeLog("system", `Starting pipeline from ${configPath}`);

    try {
      await invoke("start_pipeline", { path: configPath });
    } catch (caught) {
      const message = String(caught);
      setRuntimeStateTracked("error");
      setRuntimeEvents([]);
      setRuntimeStageStates({});
      setRuntimeStageCounters({});
      setRuntimeChannelCounters({});
      setRuntimeMessageActivity(emptyRuntimeMessageActivity);
      setRuntimeLastMessageActivity(emptyRuntimeMessageActivity);
      onError(message);
      appendRuntimeLog("system", message);
    }
  }, [appendRuntimeLog, configPath, hasUnsavedDraft, onError, setRuntimeStateTracked]);

  const stopRuntime = useCallback(async () => {
    setRuntimeStateTracked("stopping");
    appendRuntimeLog("system", "Stopping pipeline...");

    try {
      await invoke("stop_pipeline");
    } catch (caught) {
      const message = String(caught);
      setRuntimeStateTracked("error");
      onError(message);
      appendRuntimeLog("system", message);
    }
  }, [appendRuntimeLog, onError, setRuntimeStateTracked]);

  const changeRuntimeLogFilter = useCallback((filter: "all" | "selection") => {
    setRuntimeLogFilter(filter);
    if (filter === "selection") {
      setRuntimeContentFilter((contentFilter) => ({
        ...contentFilter,
        telemetry: true,
      }));
    }
  }, []);

  return {
    runtimeState,
    runtimeLogs,
    runtimeEvents,
    runtimeStageStates,
    runtimeStageCounters,
    runtimeChannelCounters,
    runtimeMessageActivity,
    runtimeLastMessageActivity,
    runtimeLogFilter,
    runtimeContentFilter,
    setRuntimeLogFilter: changeRuntimeLogFilter,
    setRuntimeContentFilter,
    startRuntime,
    stopRuntime,
    clearRuntimeLogs: () => {
      clearedAtMsRef.current = Date.now();
      discardBackendLogsUntilNextRunRef.current =
        runtimeStateRef.current !== "running" && runtimeStateRef.current !== "starting";
      setRuntimeLogs([]);
      setRuntimeEvents([]);
    },
  };
}

function formatRuntimeEvent(event: RuntimeEvent) {
  const label = event.kind.replace(/_/g, " ");
  const stage = event.stage_id ? ` ${event.stage_id}` : "";
  const processor = event.processor_type ? ` (${event.processor_type})` : "";
  const channel = event.channel_name ? ` [${event.channel_name}]` : "";
  const text = event.text ? `: ${event.text}` : "";
  return `event: ${label}${stage}${processor}${channel}${text}`;
}

function shouldShowRuntimeEventInConsole(event: RuntimeEvent) {
  return event.kind !== "message_received" && event.kind !== "message_emitted";
}

function applyRuntimeEvent(
  event: RuntimeEvent,
  setRuntimeStageStates: Dispatch<SetStateAction<RuntimeStageStates>>,
) {
  if (event.kind === "pipeline_starting") {
    setRuntimeStageStates({});
    return;
  }

  const stageId = event.stage_id;
  if (!stageId) {
    return;
  }

  const state = runtimeStageStateForEvent(event);
  if (!state) {
    return;
  }

  setRuntimeStageStates((currentStates) => {
    const currentState = currentStates[stageId];
    const nextState =
      state === "stopped" && currentState?.state === "error" ? currentState.state : state;

    return {
      ...currentStates,
      [stageId]: {
        stageId,
        processorType: event.processor_type,
        state: nextState,
        message:
          event.kind === "processor_error"
            ? event.text
            : nextState === "error"
              ? currentState?.message ?? null
              : null,
        updatedAtMs: event.timestamp_ms,
      },
    };
  });
}

function applyRuntimeMessageEvent(
  event: RuntimeEvent,
  setRuntimeMessageActivity: Dispatch<SetStateAction<RuntimeMessageActivity>>,
) {
  if (event.kind === "pipeline_starting") {
    setRuntimeMessageActivity(emptyRuntimeMessageActivity);
    return;
  }

  if (event.kind !== "message_received" && event.kind !== "message_emitted") {
    return;
  }

  setRuntimeMessageActivity((currentActivity) =>
    pruneRuntimeMessageActivity(
      {
        stageIds: event.stage_id
          ? { ...currentActivity.stageIds, [event.stage_id]: event.timestamp_ms }
          : currentActivity.stageIds,
        channelNames: event.channel_name
          ? { ...currentActivity.channelNames, [event.channel_name]: event.timestamp_ms }
          : currentActivity.channelNames,
      },
      event.timestamp_ms,
    ),
  );
}

function applyRuntimeLastMessageEvent(
  event: RuntimeEvent,
  setRuntimeLastMessageActivity: Dispatch<SetStateAction<RuntimeMessageActivity>>,
) {
  if (event.kind === "pipeline_starting") {
    setRuntimeLastMessageActivity(emptyRuntimeMessageActivity);
    return;
  }

  if (event.kind !== "message_received" && event.kind !== "message_emitted") {
    return;
  }

  setRuntimeLastMessageActivity((currentActivity) => ({
    stageIds: event.stage_id
      ? { ...currentActivity.stageIds, [event.stage_id]: event.timestamp_ms }
      : currentActivity.stageIds,
    channelNames: event.channel_name
      ? { ...currentActivity.channelNames, [event.channel_name]: event.timestamp_ms }
      : currentActivity.channelNames,
  }));
}

function applyRuntimeCounters(
  event: RuntimeEvent,
  setRuntimeStageCounters: Dispatch<SetStateAction<RuntimeStageCounters>>,
  setRuntimeChannelCounters: Dispatch<SetStateAction<RuntimeChannelCounters>>,
) {
  if (event.kind === "pipeline_starting") {
    setRuntimeStageCounters({});
    setRuntimeChannelCounters({});
    return;
  }

  const stageId = event.stage_id;
  const channelName = event.channel_name;

  if (event.kind === "processor_error" && stageId) {
    setRuntimeStageCounters((currentCounters) => {
      const current = currentCounters[stageId] ?? { received: 0, emitted: 0, errors: 0 };
      return {
        ...currentCounters,
        [stageId]: { ...current, errors: current.errors + 1 },
      };
    });
    return;
  }

  if (event.kind !== "message_received" && event.kind !== "message_emitted") {
    return;
  }

  if (stageId) {
    setRuntimeStageCounters((currentCounters) => {
      const current = currentCounters[stageId] ?? { received: 0, emitted: 0, errors: 0 };
      return {
        ...currentCounters,
        [stageId]: {
          ...current,
          received:
            event.kind === "message_received" ? current.received + 1 : current.received,
          emitted: event.kind === "message_emitted" ? current.emitted + 1 : current.emitted,
        },
      };
    });
  }

  if (channelName) {
    setRuntimeChannelCounters((currentCounters) => {
      const current = currentCounters[channelName] ?? { received: 0, emitted: 0 };
      return {
        ...currentCounters,
        [channelName]: {
          received:
            event.kind === "message_received" ? current.received + 1 : current.received,
          emitted: event.kind === "message_emitted" ? current.emitted + 1 : current.emitted,
        },
      };
    });
  }
}

function pruneRuntimeMessageActivity(
  activity: RuntimeMessageActivity,
  nowMs: number,
): RuntimeMessageActivity {
  const cutoff = nowMs - runtimeActivityTtlMs;
  const stageIds = Object.entries(activity.stageIds).filter(([, timestampMs]) => timestampMs >= cutoff);
  const channelNames = Object.entries(activity.channelNames).filter(
    ([, timestampMs]) => timestampMs >= cutoff,
  );
  // Keep the same object when nothing expired, so this 500 ms timer doesn't re-render the canvas.
  if (
    stageIds.length === Object.keys(activity.stageIds).length &&
    channelNames.length === Object.keys(activity.channelNames).length
  ) {
    return activity;
  }
  return {
    stageIds: Object.fromEntries(stageIds),
    channelNames: Object.fromEntries(channelNames),
  };
}

function runtimeStageStateForEvent(event: RuntimeEvent): RuntimeStageState | null {
  switch (event.kind) {
    case "stage_starting":
      return "starting";
    case "stage_running":
      return "running";
    case "stage_stopped":
      return "stopped";
    case "processor_error":
      return "error";
    default:
      return null;
  }
}

function applyPipelineRuntimeEvent(
  event: RuntimeEvent,
  setRuntimeState: (state: RuntimeState) => void,
) {
  switch (event.kind) {
    case "pipeline_starting":
      setRuntimeState("starting");
      break;
    case "pipeline_started":
      setRuntimeState("running");
      break;
    case "pipeline_stopped":
      setRuntimeState("idle");
      break;
    default:
      break;
  }
}
