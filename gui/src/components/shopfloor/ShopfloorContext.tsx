import { invoke } from "@tauri-apps/api/core";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { GraphNode, ResolvedPipelineGraph } from "../../types";
import type { RuntimeState } from "../runtime-console/RuntimeConsole";
import type { Focus } from "./FloorView";
import type { ShopfloorLayout, ShopfloorRequest, WorldState } from "./types";
import "./shopfloor.css";

/**
 * Shared state for `shopfloor` inputs, used by the canvas node (the floor) and the inspector (the
 * controls):
 * - the scenario layout, loaded by the GUI so the floor shows before Run;
 * - the live world, from the stage's `processor_view` runtime events while it runs;
 * - which MM / machine / sensor is selected, so a click on the canvas and the inspector agree;
 * - sending commands to the running stage (`send_stage_command`).
 */

export const SHOPFLOOR_PROCESSOR_TYPE = "shopfloor";

export function isShopfloorNode(node: GraphNode) {
  return node.processor_type === SHOPFLOOR_PROCESSOR_TYPE;
}

/** Runtime stage name of a node (`inputs.floor` → `floor`), as used in runtime events. */
export function stageNameOf(node: GraphNode) {
  const parts = node.config_path.split(".");
  return parts[parts.length - 1] || node.display_name;
}

function scenarioPathOf(node: GraphNode): string | null {
  const parameter = node.parameters.find((candidate) => candidate.key === "scenario");
  const value = parameter?.raw_value;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

type LayoutEntry = { layout: ShopfloorLayout | null; error: string | null };
type Selection = { selectedMm: string | null; focus: Focus };

interface ShopfloorContextValue {
  layouts: Record<string, LayoutEntry>;
  views: Record<string, unknown>;
  running: boolean;
  selections: Record<string, Selection>;
  updateSelection: (stage: string, update: Partial<Selection>) => void;
  send: (stage: string, request: ShopfloorRequest) => void;
}

const ShopfloorContext = createContext<ShopfloorContextValue | null>(null);

interface ShopfloorProviderProps {
  graph: ResolvedPipelineGraph | null;
  processorViews: Record<string, unknown>;
  runtimeState: RuntimeState;
  onError: (message: string | null) => void;
  children: ReactNode;
}

export function ShopfloorProvider({
  graph,
  processorViews,
  runtimeState,
  onError,
  children,
}: ShopfloorProviderProps) {
  const [layouts, setLayouts] = useState<Record<string, LayoutEntry>>({});
  const [selections, setSelections] = useState<Record<string, Selection>>({});
  const running = runtimeState === "running";

  const scenarioPaths = useMemo(() => {
    const paths = (graph?.nodes ?? [])
      .filter(isShopfloorNode)
      .map(scenarioPathOf)
      .filter((path): path is string => path !== null);
    return [...new Set(paths)].sort();
  }, [graph]);
  const scenarioPathsKey = scenarioPaths.join("|");

  // Load (or reload) layouts when the scenario files change, and when a run starts, since the
  // scenario file may have been edited in the meantime.
  useEffect(() => {
    let cancelled = false;
    for (const path of scenarioPaths) {
      invoke<ShopfloorLayout>("load_shopfloor_scenario", { path })
        .then((layout) => {
          if (!cancelled) setLayouts((current) => ({ ...current, [path]: { layout, error: null } }));
        })
        .catch((error) => {
          if (!cancelled) {
            setLayouts((current) => ({ ...current, [path]: { layout: null, error: String(error) } }));
          }
        });
    }
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scenarioPathsKey, running]);

  const updateSelection = useCallback((stage: string, update: Partial<Selection>) => {
    setSelections((current) => ({
      ...current,
      [stage]: { ...(current[stage] ?? { selectedMm: null, focus: null }), ...update },
    }));
  }, []);

  const send = useCallback(
    (stage: string, request: ShopfloorRequest) => {
      invoke("send_stage_command", { stage, command: request }).catch((error) => onError(String(error)));
    },
    [onError],
  );

  const value = useMemo(
    () => ({ layouts, views: processorViews, running, selections, updateSelection, send }),
    [layouts, processorViews, running, selections, updateSelection, send],
  );
  return <ShopfloorContext.Provider value={value}>{children}</ShopfloorContext.Provider>;
}

export interface ShopfloorStage {
  stage: string;
  scenarioPath: string | null;
  layout: ShopfloorLayout | null;
  error: string | null;
  /** Live world while/after running, otherwise the start state. */
  world: WorldState | null;
  /** Whether `world` comes from the running (or last) pipeline rather than the start state. */
  live: boolean;
  running: boolean;
  selectedMm: string | null;
  focus: Focus;
  selectMm: (id: string | null) => void;
  setFocus: (focus: Focus) => void;
  send: (request: ShopfloorRequest) => void;
}

export function useShopfloorStage(node: GraphNode): ShopfloorStage | null {
  const context = useContext(ShopfloorContext);
  if (!context) {
    return null;
  }
  const stage = stageNameOf(node);
  const scenarioPath = scenarioPathOf(node);
  const entry = scenarioPath ? context.layouts[scenarioPath] : undefined;
  const view = context.views[stage] as { state?: WorldState } | undefined;
  const liveWorld = view?.state ?? null;
  const selection = context.selections[stage] ?? { selectedMm: null, focus: null };
  return {
    stage,
    scenarioPath,
    layout: entry?.layout ?? null,
    error: scenarioPath ? (entry?.error ?? null) : "Set the `scenario` parameter to a scenario file.",
    world: liveWorld ?? entry?.layout?.state ?? null,
    live: liveWorld !== null,
    running: context.running,
    selectedMm: selection.selectedMm,
    focus: selection.focus,
    selectMm: (id) => context.updateSelection(stage, { selectedMm: id }),
    setFocus: (focus) => context.updateSelection(stage, { focus }),
    send: (request) => context.send(stage, request),
  };
}
