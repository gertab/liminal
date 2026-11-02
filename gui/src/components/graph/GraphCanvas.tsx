import {
  Background,
  BaseEdge,
  Connection,
  Controls,
  Edge,
  EdgeChange,
  EdgeLabelRenderer,
  EdgeProps,
  getBezierPath,
  Handle,
  MarkerType,
  Node,
  NodeChange,
  NodeProps,
  Position,
  ReactFlow,
  useReactFlow,
  useStore,
  useUpdateNodeInternals,
} from "@xyflow/react";
import { AlertCircle, CircleDot, Loader2, Play, RotateCcw, Square } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  RuntimeConsole,
  RuntimeContentFilter,
  RuntimeLogEntry,
  RuntimeState,
} from "../runtime-console/RuntimeConsole";
import {
  DiagnosticSeverity,
  GraphChannel,
  GraphDiagnostic,
  GraphLane,
  GraphNode,
  ResolvedPipelineGraph,
  RuntimeEvent,
  RuntimeMessageActivity,
  RuntimeStageSnapshot,
  RuntimeStageStates,
} from "../../types";

type FlowNodeData = {
  graphNode: GraphNode;
  diagnostics: GraphDiagnostic[];
  channelDiagnosticCounts: Record<string, number>;
  runtimeActive: boolean;
  runtimeSnapshot: RuntimeStageSnapshot | null;
  activeChannelNames: string[];
  connectionState: "valid" | "invalid" | null;
  selectedChannelName: string | null;
  onSelectChannel: (channelName: string) => void;
};

type FocusState = {
  activeNodeIds: Set<string>;
  activeEdgeIds: Set<string>;
};

type LayoutPosition = {
  x: number;
  y: number;
};

type FlowDebugSnapshot = {
  appNodes: number;
  renderedNodes: number;
  appEdges: number;
  renderedEdges: number;
  domNodes: number;
  domEdges: number;
  sourceHandles: number;
  targetHandles: number;
  edgeLayerChildren: number;
  storeEdges: number;
  storeEdgeLookup: number;
  storeNodes: number;
  storeWidth: number;
  storeHeight: number;
  nodesWithDimensions: number;
  nodesWithHandleBounds: number;
  flowErrors: string[];
  viewportTransform: string;
  viewportX: number;
  viewportY: number;
  zoom: number;
};

const laneTop = 24;
const nodeGap = 96;
const layoutStoragePrefix = "liminal.layout.";
const laneX: Record<GraphLane, number> = {
  inputs: 40,
  pipeline_stages: 460,
  outputs: 880,
};

const laneTitle: Record<GraphLane, string> = {
  inputs: "Inputs",
  pipeline_stages: "Pipeline Stages",
  outputs: "Outputs",
};
const nodeTypes = { liminalNode: LiminalNode };
const edgeTypes = { channelEdge: ChannelEdge };
const channelPalette = ["#67e5d8", "#8aa7ff", "#e2b24f", "#f28b82", "#b58cff", "#78d879"];

export function GraphCanvas({
  graph,
  configPath,
  selectedNodeId,
  selectedChannelName,
  selectedEdgeId,
  selectedDiagnosticKey,
  filterText,
  onSelectNode,
  onSelectChannel,
  onSelectEdge,
  onConnectNodes,
  onDisconnectEdge,
  onDeleteNode,
  onStartRuntime,
  onStopRuntime,
  error,
  loadState,
  runtimeState,
  runtimeLogs,
  runtimeEvents,
  runtimeStageStates,
  runtimeMessageActivity,
  runtimeLogFilter,
  runtimeContentFilter,
  selectedRuntimeNode,
  selectedRuntimeChannelName,
  onRuntimeLogFilterChange,
  onRuntimeContentFilterChange,
  onClearRuntimeLogs,
}: {
  graph: ResolvedPipelineGraph | null;
  configPath: string;
  selectedNodeId: string | null;
  selectedChannelName: string | null;
  selectedEdgeId: string | null;
  selectedDiagnosticKey: string | null;
  filterText: string;
  onSelectNode: (id: string | null) => void;
  onSelectChannel: (channelName: string | null) => void;
  onSelectEdge: (edgeId: string | null) => void;
  onConnectNodes: (sourceNodeId: string, targetNodeId: string) => Promise<void>;
  onDisconnectEdge: (targetNodeId: string, channelName: string) => Promise<void>;
  onDeleteNode: (nodeId: string) => Promise<void>;
  onStartRuntime: () => Promise<void>;
  onStopRuntime: () => Promise<void>;
  error: string | null;
  loadState: "idle" | "loading" | "error";
  runtimeState: RuntimeState;
  runtimeLogs: RuntimeLogEntry[];
  runtimeEvents: RuntimeEvent[];
  runtimeStageStates: RuntimeStageStates;
  runtimeMessageActivity: RuntimeMessageActivity;
  runtimeLogFilter: "all" | "selection";
  runtimeContentFilter: RuntimeContentFilter;
  selectedRuntimeNode: GraphNode | null;
  selectedRuntimeChannelName: string | null;
  onRuntimeLogFilterChange: (filter: "all" | "selection") => void;
  onRuntimeContentFilterChange: (filter: RuntimeContentFilter) => void;
  onClearRuntimeLogs: () => void;
}) {
  const [connectionSourceNodeId, setConnectionSourceNodeId] = useState<string | null>(null);
  const [layoutRevision, setLayoutRevision] = useState(0);
  const [flowRevision, setFlowRevision] = useState(0);
  const [debugExpanded, setDebugExpanded] = useState(false);
  const previousConfigPath = useRef(configPath);
  const fittedConfigPath = useRef<string | null>(null);
  const flowAreaRef = useRef<HTMLDivElement | null>(null);
  const reactFlow = useReactFlow<Node<FlowNodeData>, Edge>();
  const updateNodeInternals = useUpdateNodeInternals();
  const flowStoreSnapshotText = useStore((state) => {
    let nodesWithDimensions = 0;
    let nodesWithHandleBounds = 0;

    state.nodeLookup.forEach((node) => {
      const width = node.measured?.width ?? node.width ?? node.initialWidth;
      const height = node.measured?.height ?? node.height ?? node.initialHeight;
      if (width && height) {
        nodesWithDimensions += 1;
      }
      if (node.internals.handleBounds) {
        nodesWithHandleBounds += 1;
      }
    });

    return [
      state.edges.length,
      state.edgeLookup.size,
      state.nodes.length,
      Math.round(state.width),
      Math.round(state.height),
      nodesWithDimensions,
      nodesWithHandleBounds,
    ].join("|");
  });
  const flowStoreSnapshot = useMemo(() => {
    const [
      edges = "0",
      edgeLookup = "0",
      nodes = "0",
      width = "0",
      height = "0",
      nodesWithDimensions = "0",
      nodesWithHandleBounds = "0",
    ] = flowStoreSnapshotText.split("|");

    return {
      edgeLookup: Number(edgeLookup),
      edges: Number(edges),
      height: Number(height),
      nodes: Number(nodes),
      nodesWithDimensions: Number(nodesWithDimensions),
      nodesWithHandleBounds: Number(nodesWithHandleBounds),
      width: Number(width),
    };
  }, [flowStoreSnapshotText]);
  const savedLayout = useMemo(() => readStoredLayout(configPath), [configPath, layoutRevision]);
  const [nodePositions, setNodePositions] = useState<Record<string, LayoutPosition>>(savedLayout);
  // Sizes React Flow measured for each node. `flowNodes` is rebuilt whenever runtime activity changes
  // (several times a second while a pipeline runs); a node object without `measured` makes React Flow
  // forget the node's size and handle positions, hide the node (`visibility: hidden`) and drop its
  // edges until it re-measures on the next frame. Passing the sizes back keeps the canvas steady.
  const [nodeDimensions, setNodeDimensions] = useState<Record<string, { width: number; height: number }>>({});
  const [debugSnapshot, setDebugSnapshot] = useState<FlowDebugSnapshot | null>(null);
  const [flowErrors, setFlowErrors] = useState<string[]>([]);
  const diagnosticsByNode = useMemo(() => diagnosticsByNodeId(graph), [graph]);
  const diagnosticsByChannel = useMemo(() => diagnosticCountByChannelName(graph), [graph]);
  const focusState = useMemo(
    () => graphFocusState(graph, selectedNodeId, selectedChannelName, selectedDiagnosticKey),
    [graph, selectedChannelName, selectedDiagnosticKey, selectedNodeId],
  );
  const channelByName = useMemo(() => {
    const map = new Map<string, GraphChannel>();
    graph?.channels.forEach((channel) => map.set(channel.name, channel));
    return map;
  }, [graph]);
  const runtimeActivity = useMemo(
    () => runtimeActivityFromState(graph, runtimeStageStates, runtimeMessageActivity),
    [graph, runtimeMessageActivity, runtimeStageStates],
  );
  const runtimeSelectionTokenList = useMemo(
    () => runtimeSelectionTokens(selectedRuntimeNode, selectedRuntimeChannelName),
    [selectedRuntimeChannelName, selectedRuntimeNode],
  );
  const runtimeSelectionEventLogs = useMemo(
    () =>
      runtimeSelectionEvents(
        runtimeEvents,
        selectedRuntimeNode,
        selectedRuntimeChannelName,
      ),
    [runtimeEvents, selectedRuntimeChannelName, selectedRuntimeNode],
  );
  const runtimeTelemetryLogs = useMemo(
    () => runtimeEvents.map(runtimeEventToLogEntry),
    [runtimeEvents],
  );
  const runtimeSelectionLabel =
    selectedRuntimeChannelName ?? selectedRuntimeNode?.display_name ?? "No selection";
  const graphNodeIds = useMemo(() => graph?.nodes.map((node) => node.id) ?? [], [graph]);
  const graphNodeIdsKey = graphNodeIds.join("|");

  const flowNodes = useMemo<Node<FlowNodeData>[]>(() => {
    const query = filterText.trim().toLowerCase();
    const positions = layoutNodePositions(graph?.nodes ?? [], { ...savedLayout, ...nodePositions });

    return (graph?.nodes ?? []).map((node) => {
      const channels = [...node.input_channels, node.output_channel ?? ""]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      const matches =
        !query ||
        node.display_name.toLowerCase().includes(query) ||
        node.processor_type.toLowerCase().includes(query) ||
        channels.includes(query);
      const isFocusMuted = Boolean(focusState) && !focusState?.activeNodeIds.has(node.id);
      const channelDiagnosticCounts = [...node.input_channels, node.output_channel ?? ""]
        .filter(Boolean)
        .reduce<Record<string, number>>((counts, channelName) => {
          counts[channelName] = diagnosticsByChannel.get(channelName) ?? 0;
          return counts;
        }, {});
      const connectionState = connectionSourceNodeId
        ? connectionValidationMessage(graph, connectionSourceNodeId, node.id) === null
          ? "valid"
          : "invalid"
        : null;

      return {
        id: node.id,
        type: "liminalNode",
        position: positions.get(node.id) ?? { x: laneX[node.lane], y: laneTop },
        measured: nodeDimensions[node.id],
        draggable: true,
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        selected: node.id === selectedNodeId,
        data: {
          graphNode: node,
          diagnostics: diagnosticsByNode.get(node.id) ?? [],
          channelDiagnosticCounts,
          runtimeActive: runtimeActivity.nodeIds.has(node.id),
          runtimeSnapshot: runtimeActivity.nodeStates.get(node.id) ?? null,
          activeChannelNames: [...node.input_channels, node.output_channel ?? ""].filter((channelName) =>
            runtimeActivity.channelNames.has(channelName),
          ),
          connectionState,
          selectedChannelName,
          onSelectChannel,
        },
        className: [
          matches ? "" : "dimmed-node",
          isFocusMuted ? "focus-muted" : "",
          focusState?.activeNodeIds.has(node.id) ? "focus-active" : "",
        ]
          .filter(Boolean)
          .join(" "),
      };
    });
  }, [
    diagnosticsByChannel,
    diagnosticsByNode,
    filterText,
    focusState,
    graph,
    connectionSourceNodeId,
    onSelectChannel,
    nodeDimensions,
    nodePositions,
    runtimeActivity,
    savedLayout,
    selectedChannelName,
    selectedNodeId,
  ]);

  const flowEdges = useMemo<Edge[]>(() => {
    return (graph?.edges ?? []).map((edge, index) => {
      const channel = channelByName.get(edge.channel_name);
      const color = colorForChannel(edge.channel_name);
      const laneOffset = ((index % 5) - 2) * 14;
      const channelDiagnostics =
        graph?.diagnostics.filter((diagnostic) => diagnostic.channel_name === edge.channel_name) ?? [];
      const pathState = focusState
        ? focusState.activeEdgeIds.has(edge.id)
          ? "active"
          : "muted"
        : null;

      return {
        id: edge.id,
        source: edge.source_node_id,
        target: edge.target_node_id,
        sourceHandle: "output-channel",
        targetHandle: `input-channel-${edge.target_input_index}`,
        type: "channelEdge",
        selected: edge.id === selectedEdgeId,
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color,
          width: 16,
          height: 16,
        },
        label: edge.channel_name,
        data: {
          channelName: edge.channel_name,
          color,
          laneOffset,
          onSelectEdge,
          pathState,
          severity: severityForDiagnostics(channelDiagnostics),
          runtimeActive: runtimeActivity.channelNames.has(edge.channel_name),
        },
        animated: channel?.channel_type === "broadcast" || channel?.channel_type === "fanout",
        style: { stroke: color, strokeWidth: 2.1 },
      };
    });
  }, [channelByName, focusState, graph, onSelectEdge, runtimeActivity, selectedEdgeId]);
  const flowKey = `${configPath}:${flowRevision}`;
  const updateDebugSnapshot = useCallback(() => {
    const viewport = reactFlow.getViewport();
    const flowArea = flowAreaRef.current;
    const viewportElement = flowArea?.querySelector<HTMLElement>(".react-flow__viewport") ?? null;
    const edgeLayer = flowArea?.querySelector<HTMLElement>(".react-flow__edges") ?? null;
    const domNodes = flowArea?.querySelectorAll(".react-flow__node").length ?? 0;
    const domEdges = flowArea?.querySelectorAll(".react-flow__edge").length ?? 0;
    const sourceHandles =
      flowArea?.querySelectorAll(".react-flow__handle.source, .xyflow__handle.source").length ?? 0;
    const targetHandles =
      flowArea?.querySelectorAll(".react-flow__handle.target, .xyflow__handle.target").length ?? 0;

    setDebugSnapshot({
      appNodes: flowNodes.length,
      renderedNodes: reactFlow.getNodes().length,
      appEdges: flowEdges.length,
      renderedEdges: reactFlow.getEdges().length,
      domNodes,
      domEdges,
      sourceHandles,
      targetHandles,
      edgeLayerChildren: edgeLayer?.children.length ?? 0,
      storeEdges: flowStoreSnapshot.edges,
      storeEdgeLookup: flowStoreSnapshot.edgeLookup,
      storeNodes: flowStoreSnapshot.nodes,
      storeWidth: flowStoreSnapshot.width,
      storeHeight: flowStoreSnapshot.height,
      nodesWithDimensions: flowStoreSnapshot.nodesWithDimensions,
      nodesWithHandleBounds: flowStoreSnapshot.nodesWithHandleBounds,
      flowErrors,
      viewportTransform: viewportElement?.style.transform || "none",
      viewportX: Math.round(viewport.x),
      viewportY: Math.round(viewport.y),
      zoom: Number(viewport.zoom.toFixed(3)),
    });
  }, [flowEdges.length, flowErrors, flowNodes.length, flowStoreSnapshot, reactFlow]);

  const recordFlowError = useCallback((id: string, message: string) => {
    setFlowErrors((currentErrors) => {
      const nextError = `${id}: ${message}`;
      return [nextError, ...currentErrors.filter((error) => error !== nextError)].slice(0, 5);
    });
  }, []);

  const refreshFlowGeometry = useCallback(() => {
    if (graphNodeIds.length === 0) {
      return;
    }

    updateNodeInternals(graphNodeIds);
    window.setTimeout(updateDebugSnapshot, 80);
  }, [graphNodeIds, updateDebugSnapshot, updateNodeInternals]);

  const hasMissingEdgeDom = useCallback(() => {
    const flowArea = flowAreaRef.current;
    if (!flowArea || flowEdges.length === 0) {
      return false;
    }

    const domNodes = flowArea.querySelectorAll(".react-flow__node").length;
    const domEdges = flowArea.querySelectorAll(".react-flow__edge").length;
    return domNodes > 0 && domEdges < flowEdges.length;
  }, [flowEdges.length]);

  const repairMissingEdgeDom = useCallback(() => {
    if (hasMissingEdgeDom()) {
      refreshFlowGeometry();
    } else {
      updateDebugSnapshot();
    }
  }, [hasMissingEdgeDom, refreshFlowGeometry, updateDebugSnapshot]);

  const recoverFlowView = useCallback(() => {
    setNodeDimensions({});
    setFlowRevision((revision) => revision + 1);
    fittedConfigPath.current = null;
    window.requestAnimationFrame(() => {
      reactFlow.fitView({ padding: 0.18, duration: 180 });
      window.setTimeout(updateDebugSnapshot, 220);
    });
  }, [reactFlow, updateDebugSnapshot]);

  useEffect(() => {
    window.requestAnimationFrame(refreshFlowGeometry);
  }, [configPath, graphNodeIdsKey, refreshFlowGeometry]);

  useEffect(() => {
    let frame: number | null = null;
    const flowArea = flowAreaRef.current;

    const scheduleGeometryRefresh = () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }

      frame = window.requestAnimationFrame(() => {
        frame = null;
        refreshFlowGeometry();
      });
    };

    window.addEventListener("resize", scheduleGeometryRefresh);
    const resizeObserver = new ResizeObserver(scheduleGeometryRefresh);
    if (flowArea) {
      resizeObserver.observe(flowArea);
    }

    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      window.removeEventListener("resize", scheduleGeometryRefresh);
      resizeObserver.disconnect();
    };
  }, [refreshFlowGeometry]);

  useEffect(() => {
    let frame: number | null = null;

    const scheduleMissingEdgeRepair = () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }

      frame = window.requestAnimationFrame(() => {
        frame = null;
        repairMissingEdgeDom();
      });
    };

    window.addEventListener("scroll", scheduleMissingEdgeRepair, true);
    const interval = window.setInterval(repairMissingEdgeDom, 500);
    return () => {
      if (frame !== null) {
        window.cancelAnimationFrame(frame);
      }
      window.removeEventListener("scroll", scheduleMissingEdgeRepair, true);
      window.clearInterval(interval);
    };
  }, [repairMissingEdgeDom]);

  useEffect(() => {
    const toggleFlowDebug = (event: KeyboardEvent) => {
      if (!event.shiftKey || !(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "d") {
        return;
      }

      if (isTextEditingTarget(event.target)) {
        return;
      }

      event.preventDefault();
      setDebugExpanded((expanded) => !expanded);
    };

    window.addEventListener("keydown", toggleFlowDebug);
    return () => window.removeEventListener("keydown", toggleFlowDebug);
  }, []);

  const onNodesChange = useCallback(
    (changes: NodeChange<Node<FlowNodeData>>[]) => {
      const removeChanges = changes.filter((change) => change.type === "remove");
      const positionChanges = changes.filter((change) => change.type === "position" && change.position);
      const dimensionChanges = changes.flatMap((change) =>
        change.type === "dimensions" && change.dimensions ? [{ id: change.id, ...change.dimensions }] : [],
      );

      if (dimensionChanges.length > 0) {
        setNodeDimensions((current) => {
          let next = current;
          dimensionChanges.forEach(({ id, width, height }) => {
            if (current[id]?.width !== width || current[id]?.height !== height) {
              next = next === current ? { ...current } : next;
              next[id] = { width, height };
            }
          });
          return next;
        });
      }

      removeChanges.forEach((change) => {
        onDeleteNode(change.id);
      });

      if (positionChanges.length > 0) {
        setNodePositions((currentPositions) => {
          const nextPositions = { ...currentPositions };
          positionChanges.forEach((change) => {
            if (change.type === "position" && change.position) {
              nextPositions[change.id] = {
                x: Math.round(change.position.x),
                y: Math.round(change.position.y),
              };
            }
          });
          return nextPositions;
        });
      }
    },
    [onDeleteNode],
  );
  const onEdgesChange = useCallback((_changes: EdgeChange[]) => {}, []);
  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) {
        return;
      }

      onConnectNodes(connection.source, connection.target);
    },
    [onConnectNodes],
  );
  const isValidConnection = useCallback(
    (connection: Connection | Edge) =>
      Boolean(connection.source && connection.target) &&
      connectionValidationMessage(graph, connection.source ?? "", connection.target ?? "") === null,
    [graph],
  );

  useEffect(() => {
    if (previousConfigPath.current !== configPath) {
      previousConfigPath.current = configPath;
      setNodePositions(savedLayout);
      setNodeDimensions({});
      setFlowErrors([]);
      fittedConfigPath.current = null;
    }
  }, [configPath, savedLayout]);

  useEffect(() => {
    if (!graph || flowNodes.length === 0) {
      return;
    }

    if (fittedConfigPath.current === configPath) {
      return;
    }

    fittedConfigPath.current = configPath;
    window.requestAnimationFrame(() => {
      reactFlow.fitView({ padding: 0.18, duration: 180 });
    });
  }, [configPath, flowNodes.length, graph, reactFlow]);

  useEffect(() => {
    if (graph) {
      pruneStoredLayout(configPath, graph.nodes);
    }
  }, [configPath, graph]);

  useEffect(() => {
    updateDebugSnapshot();
    const interval = window.setInterval(updateDebugSnapshot, 500);
    return () => window.clearInterval(interval);
  }, [updateDebugSnapshot]);

  if (loadState === "error") {
    return (
      <div className="center-message error-message">
        <AlertCircle size={28} />
        <p>{error}</p>
      </div>
    );
  }

  if (!graph) {
    return (
      <div className="center-message">
        <Loader2 className="spin" size={28} />
      </div>
    );
  }

  return (
    <div className="canvas-shell">
      <LaneLabels graph={graph} />
      <div className="layout-toolbar">
        <button
          className={runtimeState === "running" ? "runtime-button running" : "runtime-button"}
          onClick={runtimeState === "running" || runtimeState === "stopping" ? onStopRuntime : onStartRuntime}
          disabled={runtimeState === "starting" || runtimeState === "stopping"}
          title={runtimeState === "running" ? "Stop pipeline" : "Run pipeline"}
          aria-label={runtimeState === "running" ? "Stop pipeline" : "Run pipeline"}
        >
          {runtimeState === "running" || runtimeState === "stopping" ? (
            <Square size={15} />
          ) : (
            <Play size={15} />
          )}
        </button>
        <button
          onClick={() => {
            clearStoredLayout(configPath);
            setNodePositions({});
            setLayoutRevision((revision) => revision + 1);
            window.requestAnimationFrame(() => {
              reactFlow.fitView({ padding: 0.18, duration: 180 });
            });
          }}
          title="Reset node layout"
          aria-label="Reset node layout"
        >
          <RotateCcw size={15} />
        </button>
        <button
          className={debugExpanded ? "debug-toolbar-button active" : "debug-toolbar-button"}
          onClick={() => setDebugExpanded((expanded) => !expanded)}
          title={debugExpanded ? "Hide flow debug (Cmd/Ctrl+Shift+D)" : "Show flow debug (Cmd/Ctrl+Shift+D)"}
          aria-label={debugExpanded ? "Hide flow debug" : "Show flow debug"}
        >
          Debug
        </button>
        <button
          className="debug-toolbar-button"
          onClick={recoverFlowView}
          title="Recover graph renderer"
          aria-label="Recover graph renderer"
        >
          Recover
        </button>
      </div>
      <RuntimeConsole
        logs={runtimeLogs}
        state={runtimeState}
        filter={runtimeLogFilter}
        contentFilter={runtimeContentFilter}
        selectionTokens={runtimeSelectionTokenList}
        telemetryLogs={runtimeTelemetryLogs}
        selectionEventLogs={runtimeSelectionEventLogs}
        selectionLabel={runtimeSelectionLabel}
        onFilterChange={onRuntimeLogFilterChange}
        onContentFilterChange={onRuntimeContentFilterChange}
        onClear={onClearRuntimeLogs}
      />
      <div className="flow-area" ref={flowAreaRef}>
        <ReactFlow
          key={flowKey}
          nodes={flowNodes}
          edges={flowEdges}
          nodeTypes={nodeTypes}
          edgeTypes={edgeTypes}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onError={recordFlowError}
          onConnect={onConnect}
          isValidConnection={isValidConnection}
          onConnectStart={(_, params) => setConnectionSourceNodeId(params.nodeId ?? null)}
          onConnectEnd={() => setConnectionSourceNodeId(null)}
          onEdgesDelete={(deletedEdges) => {
            deletedEdges.forEach((edge) => {
              const channelName = (edge.data as ChannelEdgeData | undefined)?.channelName;
              if (edge.target && channelName) {
                onDisconnectEdge(edge.target, channelName);
              }
            });
          }}
          onNodeDragStop={(_, node) => {
            setNodePositions((currentPositions) => {
              const nextPositions = {
                ...currentPositions,
                [node.id]: {
                  x: Math.round(node.position.x),
                  y: Math.round(node.position.y),
                },
              };
              writeStoredNodePositions(configPath, flowNodes, nextPositions);
              return nextPositions;
            });
            setLayoutRevision((revision) => revision + 1);
          }}
          onNodeClick={(_, node) => {
            onSelectNode(node.id);
          }}
          onEdgeClick={(_, edge) => {
            onSelectEdge(edge.id);
          }}
          onPaneClick={() => {
            onSelectNode(null);
          }}
          onlyRenderVisibleElements={false}
          colorMode="dark"
          nodesDraggable
          nodesConnectable
          elementsSelectable
        >
          <Background color="#243235" gap={24} size={1} />
          <Controls showInteractive={false} />
        </ReactFlow>
      </div>
      {debugExpanded && (
        <FlowDebugOverlay
          snapshot={debugSnapshot}
          configPath={configPath}
          flowKey={flowKey}
          runtimeState={runtimeState}
          onClose={() => setDebugExpanded(false)}
          onRecover={recoverFlowView}
        />
      )}
    </div>
  );
}

function FlowDebugOverlay({
  snapshot,
  configPath,
  flowKey,
  runtimeState,
  onClose,
  onRecover,
}: {
  snapshot: FlowDebugSnapshot | null;
  configPath: string;
  flowKey: string;
  runtimeState: RuntimeState;
  onClose: () => void;
  onRecover: () => void;
}) {
  return (
    <aside className="flow-debug-overlay">
      <div className="flow-debug-title">
        <button onClick={onClose}>Hide</button>
        <span>Flow Debug</span>
        <button onClick={onRecover}>Recover</button>
      </div>
      <dl>
        <div>
          <dt>App nodes</dt>
          <dd>{snapshot?.appNodes ?? "-"}</dd>
        </div>
        <div>
          <dt>Flow nodes</dt>
          <dd>{snapshot?.renderedNodes ?? "-"}</dd>
        </div>
        <div>
          <dt>App edges</dt>
          <dd>{snapshot?.appEdges ?? "-"}</dd>
        </div>
        <div>
          <dt>Flow edges</dt>
          <dd>{snapshot?.renderedEdges ?? "-"}</dd>
        </div>
        <div>
          <dt>DOM nodes</dt>
          <dd>{snapshot?.domNodes ?? "-"}</dd>
        </div>
        <div>
          <dt>DOM edges</dt>
          <dd>{snapshot?.domEdges ?? "-"}</dd>
        </div>
        <div>
          <dt>Edge layer children</dt>
          <dd>{snapshot?.edgeLayerChildren ?? "-"}</dd>
        </div>
        <div>
          <dt>Store edges</dt>
          <dd>{snapshot?.storeEdges ?? "-"}</dd>
        </div>
        <div>
          <dt>Store edge lookup</dt>
          <dd>{snapshot?.storeEdgeLookup ?? "-"}</dd>
        </div>
        <div>
          <dt>Store nodes</dt>
          <dd>{snapshot?.storeNodes ?? "-"}</dd>
        </div>
        <div>
          <dt>Store size</dt>
          <dd>
            {snapshot
              ? `${snapshot.storeWidth} x ${snapshot.storeHeight}`
              : "-"}
          </dd>
        </div>
        <div>
          <dt>Source handles</dt>
          <dd>{snapshot?.sourceHandles ?? "-"}</dd>
        </div>
        <div>
          <dt>Target handles</dt>
          <dd>{snapshot?.targetHandles ?? "-"}</dd>
        </div>
        <div>
          <dt>Measured nodes</dt>
          <dd>{snapshot?.nodesWithDimensions ?? "-"}</dd>
        </div>
        <div>
          <dt>Handle-bound nodes</dt>
          <dd>{snapshot?.nodesWithHandleBounds ?? "-"}</dd>
        </div>
        <div>
          <dt>Viewport</dt>
          <dd>
            {snapshot
              ? `${snapshot.viewportX}, ${snapshot.viewportY} @ ${snapshot.zoom}`
              : "-"}
          </dd>
        </div>
        <div>
          <dt>Transform</dt>
          <dd title={snapshot?.viewportTransform ?? ""}>{snapshot?.viewportTransform ?? "-"}</dd>
        </div>
        <div>
          <dt>Runtime</dt>
          <dd>{runtimeState}</dd>
        </div>
        <div>
          <dt>Config</dt>
          <dd title={configPath}>{configPath}</dd>
        </div>
        <div>
          <dt>Flow key</dt>
          <dd title={flowKey}>{flowKey}</dd>
        </div>
        <div>
          <dt>Flow errors</dt>
          <dd title={snapshot?.flowErrors.join("\n") ?? ""}>
            {snapshot?.flowErrors.length ? snapshot.flowErrors.join(" | ") : "none"}
          </dd>
        </div>
      </dl>
    </aside>
  );
}

function LiminalNode({ data, selected }: NodeProps<Node<FlowNodeData>>) {
  const {
    graphNode,
    diagnostics,
    channelDiagnosticCounts,
    runtimeActive,
    runtimeSnapshot,
    activeChannelNames,
    connectionState,
    selectedChannelName,
    onSelectChannel,
  } = data;
  const hasError = diagnostics.some((diagnostic) => diagnostic.severity === "error");
  const hasWarning = diagnostics.some((diagnostic) => diagnostic.severity === "warning");
  const diagnosticSeverity = severityForDiagnostics(diagnostics);
  const className = [
    "liminal-node",
    graphNode.kind,
    selected ? "selected" : "",
    connectionState === "valid" ? "connection-valid" : "",
    connectionState === "invalid" ? "connection-invalid" : "",
    runtimeActive ? "runtime-active" : "",
    runtimeSnapshot?.state === "error" ? "runtime-error" : "",
    hasError ? "has-error" : "",
    hasWarning ? "has-warning" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const outputChannel = graphNode.output_channel;

  return (
    <div className={className}>
      <div className="node-header">
        <CircleDot size={14} />
        <span>{graphNode.processor_type}</span>
        {runtimeSnapshot && (
          <span
            className={`runtime-node-badge ${runtimeSnapshot.state}`}
            title={runtimeSnapshot.message ?? runtimeSnapshot.state}
          >
            {runtimeSnapshot.state}
          </span>
        )}
        {diagnosticSeverity && <DiagnosticBadge count={diagnostics.length} severity={diagnosticSeverity} />}
      </div>
      <strong>{graphNode.display_name}</strong>
      <p>{graphNode.config_path}</p>
      <div className="node-channels">
        {graphNode.kind !== "input" && (
          <Handle
            className="node-input-drop-handle"
            id="input-add"
            type="target"
            position={Position.Left}
            isConnectable
          />
        )}
        {graphNode.input_channels.map((channel, index) => (
          <button
            key={`${channel}-${index}`}
            className={channelClassName("input-channel", channel, selectedChannelName, activeChannelNames)}
            title={channel}
            onClick={(event) => {
              event.stopPropagation();
              onSelectChannel(channel);
            }}
          >
            <Handle
              className="channel-handle input-channel-handle"
              id={`input-channel-${index}`}
              type="target"
              position={Position.Left}
              isConnectable
            />
            <span className="channel-text">{channel}</span>
            {(channelDiagnosticCounts[channel] ?? 0) > 0 && (
              <DiagnosticBadge count={channelDiagnosticCounts[channel]} severity="warning" compact />
            )}
          </button>
        ))}
        {outputChannel && (
          <button
            className={channelClassName("output-channel", outputChannel, selectedChannelName, activeChannelNames)}
            title={outputChannel}
            onClick={(event) => {
              event.stopPropagation();
              onSelectChannel(outputChannel);
            }}
          >
            <span className="channel-text">{outputChannel}</span>
            <Handle
              className="channel-handle output-channel-handle"
              id="output-channel"
              type="source"
              position={Position.Right}
              isConnectable
            />
            {(channelDiagnosticCounts[outputChannel] ?? 0) > 0 && (
              <DiagnosticBadge count={channelDiagnosticCounts[outputChannel]} severity="warning" compact />
            )}
          </button>
        )}
      </div>
    </div>
  );
}

type ChannelEdgeData = {
  channelName: string;
  color: string;
  laneOffset: number;
  onSelectEdge?: (edgeId: string) => void;
  pathState: "active" | "muted" | null;
  severity: DiagnosticSeverity | null;
  runtimeActive: boolean;
};

function ChannelEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  style,
  selected,
  data,
}: EdgeProps<Edge<ChannelEdgeData>>) {
  const edgeData = data ?? {
    channelName: "",
    color: "#67e5d8",
    laneOffset: 0,
    onSelectEdge: undefined,
    pathState: null,
    severity: null,
    runtimeActive: false,
  };
  const [basePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    curvature: 0.34,
  });
  const edgePath =
    edgeData.laneOffset === 0
      ? basePath
      : channelBezierPath(sourceX, sourceY, targetX, targetY, edgeData.laneOffset);

  return (
    <>
      <BaseEdge
        id={id}
        path={edgePath}
        markerEnd={markerEnd}
        className={[
          "channel-edge",
          selected ? "selected" : "",
          edgeData.severity ? edgeData.severity : "",
          edgeData.pathState === "active" ? "focus-active" : "",
          edgeData.pathState === "muted" ? "focus-muted" : "",
          edgeData.runtimeActive ? "runtime-active" : "",
        ]
          .filter(Boolean)
          .join(" ")}
        style={{
          ...style,
          stroke: edgeData.color,
        }}
      />
      <EdgeLabelRenderer>
        <div
          className={[
            "edge-label",
            "nodrag",
            "nopan",
            edgeData.severity ? edgeData.severity : "",
            edgeData.pathState === "active" ? "focus-active" : "",
            edgeData.pathState === "muted" ? "focus-muted" : "",
            edgeData.runtimeActive ? "runtime-active" : "",
          ]
            .filter(Boolean)
            .join(" ")}
          style={{
            left: labelX,
            top: labelY,
            borderColor: edgeData.color,
            color: edgeData.color,
          }}
          title={edgeData.channelName}
          onClick={(event) => {
            event.stopPropagation();
            edgeData.onSelectEdge?.(id);
          }}
        >
          {edgeData.channelName}
        </div>
      </EdgeLabelRenderer>
    </>
  );
}

function channelBezierPath(
  sourceX: number,
  sourceY: number,
  targetX: number,
  targetY: number,
  laneOffset: number,
) {
  const distance = Math.max(Math.abs(targetX - sourceX), 160);
  const controlDistance = Math.min(distance * 0.42, 260);
  const controlSourceX = sourceX + controlDistance;
  const controlTargetX = targetX - controlDistance;
  const controlSourceY = sourceY + laneOffset;
  const controlTargetY = targetY - laneOffset;

  return `M ${sourceX},${sourceY} C ${controlSourceX},${controlSourceY} ${controlTargetX},${controlTargetY} ${targetX},${targetY}`;
}

function channelClassName(
  kindClass: string,
  channelName: string,
  selectedChannelName: string | null,
  activeChannelNames: string[] = [],
) {
  return [
    "channel",
    "nodrag",
    kindClass,
    channelName === selectedChannelName ? "selected-channel" : "",
    activeChannelNames.includes(channelName) ? "runtime-active" : "",
  ]
    .filter(Boolean)
    .join(" ");
}

function DiagnosticBadge({
  count,
  severity,
  compact = false,
}: {
  count: number;
  severity: DiagnosticSeverity;
  compact?: boolean;
}) {
  return <span className={`diagnostic-badge ${severity} ${compact ? "compact" : ""}`}>{count}</span>;
}

function colorForChannel(channelName: string) {
  let hash = 0;
  for (const character of channelName) {
    hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  }

  return channelPalette[hash % channelPalette.length];
}

function LaneLabels({ graph }: { graph: ResolvedPipelineGraph }) {
  return (
    <div className="lane-labels">
      {(["inputs", "pipeline_stages", "outputs"] as GraphLane[]).map((lane) => {
        const diagnostics = diagnosticsForLane(graph, lane);
        const severity = severityForDiagnostics(diagnostics);

        return (
        <div key={lane}>
          <span>{laneTitle[lane]}</span>
          {severity && <DiagnosticBadge count={diagnostics.length} severity={severity} />}
        </div>
        );
      })}
    </div>
  );
}

function diagnosticsByNodeId(graph: ResolvedPipelineGraph | null) {
  const map = new Map<string, GraphDiagnostic[]>();

  graph?.diagnostics.forEach((diagnostic) => {
    diagnostic.node_ids.forEach((nodeId) => {
      const diagnostics = map.get(nodeId) ?? [];
      diagnostics.push(diagnostic);
      map.set(nodeId, diagnostics);
    });
  });

  return map;
}

function diagnosticCountByChannelName(graph: ResolvedPipelineGraph | null) {
  const map = new Map<string, number>();

  graph?.diagnostics.forEach((diagnostic) => {
    if (!diagnostic.channel_name) {
      return;
    }

    map.set(diagnostic.channel_name, (map.get(diagnostic.channel_name) ?? 0) + 1);
  });

  return map;
}

function diagnosticsForLane(graph: ResolvedPipelineGraph, lane: GraphLane) {
  const laneNodeIds = new Set(graph.nodes.filter((node) => node.lane === lane).map((node) => node.id));

  return graph.diagnostics.filter((diagnostic) =>
    diagnostic.node_ids.some((nodeId) => laneNodeIds.has(nodeId)),
  );
}

function severityForDiagnostics(diagnostics: GraphDiagnostic[]) {
  if (diagnostics.some((diagnostic) => diagnostic.severity === "error")) {
    return "error";
  }

  if (diagnostics.some((diagnostic) => diagnostic.severity === "warning")) {
    return "warning";
  }

  return null;
}

function diagnosticKey(diagnostic: GraphDiagnostic, index: number) {
  return [
    index,
    diagnostic.kind,
    diagnostic.severity,
    diagnostic.channel_name ?? "",
    diagnostic.node_ids.join(","),
  ].join(":");
}

export function connectionValidationMessage(
  graph: ResolvedPipelineGraph | null,
  sourceNodeId: string,
  targetNodeId: string,
) {
  if (!graph) {
    return "No graph is loaded.";
  }

  if (!sourceNodeId || !targetNodeId) {
    return "Select a source and target node.";
  }

  if (sourceNodeId === targetNodeId) {
    return "A node cannot be connected to itself.";
  }

  const sourceNode = graph.nodes.find((node) => node.id === sourceNodeId);
  const targetNode = graph.nodes.find((node) => node.id === targetNodeId);

  if (!sourceNode) {
    return `Source node '${sourceNodeId}' was not found.`;
  }

  if (!targetNode) {
    return `Target node '${targetNodeId}' was not found.`;
  }

  if (targetNode.kind === "input") {
    return "Input stages cannot consume channels.";
  }

  if (!sourceNode.output_channel) {
    return `Source node '${sourceNode.display_name}' does not produce an output channel.`;
  }

  if (targetNode.input_channels.includes(sourceNode.output_channel)) {
    return `Target node '${targetNode.display_name}' already consumes '${sourceNode.output_channel}'.`;
  }

  return null;
}

function graphFocusState(
  graph: ResolvedPipelineGraph | null,
  selectedNodeId: string | null,
  selectedChannelName: string | null,
  selectedDiagnosticKey: string | null,
): FocusState | null {
  if (!graph) {
    return null;
  }

  const selectedDiagnostic =
    graph.diagnostics.find((diagnostic, index) => diagnosticKey(diagnostic, index) === selectedDiagnosticKey) ??
    null;

  if (selectedChannelName || selectedDiagnostic?.channel_name) {
    const channelName = selectedChannelName ?? selectedDiagnostic?.channel_name;
    if (!channelName) {
      return null;
    }
    const activeEdgeIds = new Set(
      graph.edges
        .filter((edge) => edge.channel_name === channelName)
        .map((edge) => edge.id),
    );
    const activeNodeIds = new Set<string>();

    graph.edges
      .filter((edge) => edge.channel_name === channelName)
      .forEach((edge) => {
        activeNodeIds.add(edge.source_node_id);
        activeNodeIds.add(edge.target_node_id);
      });

    const channel = graph.channels.find((candidate) => candidate.name === channelName);
    channel?.producer_node_ids.forEach((nodeId) => activeNodeIds.add(nodeId));
    channel?.consumer_node_ids.forEach((nodeId) => activeNodeIds.add(nodeId));
    selectedDiagnostic?.node_ids.forEach((nodeId) => activeNodeIds.add(nodeId));

    return { activeNodeIds, activeEdgeIds };
  }

  const focusNodeId = selectedNodeId ?? selectedDiagnostic?.node_ids[0] ?? null;
  if (!focusNodeId) {
    return null;
  }

  const activeNodeIds = new Set<string>([focusNodeId]);
  const activeEdgeIds = new Set<string>();
  const frontier = [focusNodeId];

  while (frontier.length > 0) {
    const nodeId = frontier.shift();
    if (!nodeId) {
      continue;
    }

    graph.edges
      .filter((edge) => edge.source_node_id === nodeId || edge.target_node_id === nodeId)
      .forEach((edge) => {
        const nextNodeId = edge.source_node_id === nodeId ? edge.target_node_id : edge.source_node_id;
        activeEdgeIds.add(edge.id);

        if (!activeNodeIds.has(nextNodeId)) {
          activeNodeIds.add(nextNodeId);
          frontier.push(nextNodeId);
        }
      });
  }

  return { activeNodeIds, activeEdgeIds };
}

function runtimeSelectionTokens(selectedNode: GraphNode | null, selectedChannelName: string | null) {
  const tokens = new Set<string>();

  if (selectedChannelName) {
    tokens.add(selectedChannelName);
  }

  if (selectedNode) {
    tokens.add(selectedNode.id);
    tokens.add(selectedNode.display_name);
    tokens.add(selectedNode.config_path);
    selectedNode.input_channels.forEach((channelName) => tokens.add(channelName));
    if (selectedNode.output_channel) {
      tokens.add(selectedNode.output_channel);
    }
  }

  return [...tokens].filter((token) => token.trim().length > 0);
}

function runtimeSelectionEvents(
  events: RuntimeEvent[],
  selectedNode: GraphNode | null,
  selectedChannelName: string | null,
): RuntimeLogEntry[] {
  if (!selectedNode && !selectedChannelName) {
    return [];
  }

  const selectedStageName = selectedNode ? stageNameForNode(selectedNode) : null;
  const selectedNodeChannels = new Set<string>(
    selectedNode
      ? [...selectedNode.input_channels, selectedNode.output_channel ?? ""].filter(Boolean)
      : [],
  );

  return events
    .filter((event) => {
      if (selectedChannelName) {
        return event.channel_name === selectedChannelName;
      }

      return (
        Boolean(selectedStageName && event.stage_id === selectedStageName) ||
        Boolean(event.channel_name && selectedNodeChannels.has(event.channel_name))
      );
    })
    .map(runtimeEventToLogEntry);
}

function runtimeEventToLogEntry(event: RuntimeEvent): RuntimeLogEntry {
  return {
    id: event.id,
    stream: "system",
    line: formatRuntimeSelectionEvent(event),
    timestampMs: event.timestamp_ms,
  };
}

function formatRuntimeSelectionEvent(event: RuntimeEvent) {
  const label = event.kind.replace(/_/g, " ");
  const stage = event.stage_id ? ` ${event.stage_id}` : "";
  const processor = event.processor_type ? ` (${event.processor_type})` : "";
  const channel = event.channel_name ? ` [${event.channel_name}]` : "";
  const text = event.text ? `: ${event.text}` : "";
  return `event: ${label}${stage}${processor}${channel}${text}`;
}

function runtimeActivityFromState(
  graph: ResolvedPipelineGraph | null,
  stageStates: RuntimeStageStates,
  messageActivity: RuntimeMessageActivity,
) {
  const activeNodeIds = new Set<string>();
  const activeChannelNames = new Set<string>();
  const nodeStates = new Map<string, RuntimeStageSnapshot>();

  if (!graph) {
    return { nodeIds: activeNodeIds, channelNames: activeChannelNames, nodeStates };
  }

  graph.nodes.forEach((node) => {
    const stageName = stageNameForNode(node);
    const runtimeSnapshot = stageName ? stageStates[stageName] : null;
    if (!runtimeSnapshot) {
      return;
    }

    nodeStates.set(node.id, runtimeSnapshot);
  });

  Object.keys(messageActivity.channelNames).forEach((channelName) =>
    activeChannelNames.add(channelName),
  );

  graph.nodes.forEach((node) => {
    const stageName = stageNameForNode(node);
    if (stageName && messageActivity.stageIds[stageName]) {
      activeNodeIds.add(node.id);
    }
  });

  graph.edges.forEach((edge) => {
    if (activeChannelNames.has(edge.channel_name)) {
      activeNodeIds.add(edge.source_node_id);
      activeNodeIds.add(edge.target_node_id);
    }
  });

  return { nodeIds: activeNodeIds, channelNames: activeChannelNames, nodeStates };
}

function stageNameForNode(node: GraphNode) {
  const configPathParts = node.config_path.split(".");
  const lastConfigPathPart = configPathParts[configPathParts.length - 1];
  if (lastConfigPathPart) {
    return lastConfigPathPart;
  }

  if (node.id.startsWith("input:") || node.id.startsWith("output:")) {
    const idParts = node.id.split(":");
    return idParts[idParts.length - 1] ?? null;
  }

  const stageMarker = ".stage:";
  const stageMarkerIndex = node.id.indexOf(stageMarker);
  if (stageMarkerIndex >= 0) {
    return node.id.slice(stageMarkerIndex + stageMarker.length);
  }

  return null;
}

function isTextEditingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    target.isContentEditable
  );
}

function readStoredLayout(configPath: string): Record<string, LayoutPosition> {
  try {
    const storedLayout = window.localStorage.getItem(layoutStorageKey(configPath));
    if (!storedLayout) {
      return {};
    }

    const parsedLayout = JSON.parse(storedLayout) as Record<string, LayoutPosition>;
    return Object.fromEntries(
      Object.entries(parsedLayout).filter(
        ([, position]) =>
          Number.isFinite(position?.x) &&
          Number.isFinite(position?.y),
      ),
    );
  } catch {
    return {};
  }
}

function writeStoredNodePositions(
  configPath: string,
  nodes: Node<FlowNodeData>[],
  positions: Record<string, LayoutPosition>,
) {
  const layout = Object.fromEntries(
    nodes.map((node) => [
      node.id,
      {
        x: Math.round(positions[node.id]?.x ?? node.position.x),
        y: Math.round(positions[node.id]?.y ?? node.position.y),
      },
    ]),
  );

  window.localStorage.setItem(layoutStorageKey(configPath), JSON.stringify(layout));
}

function pruneStoredLayout(configPath: string, nodes: GraphNode[]) {
  const layout = readStoredLayout(configPath);
  const nodeIds = new Set(nodes.map((node) => node.id));
  const prunedLayout = Object.fromEntries(
    Object.entries(layout).filter(([nodeId]) => nodeIds.has(nodeId)),
  );

  if (Object.keys(prunedLayout).length !== Object.keys(layout).length) {
    window.localStorage.setItem(layoutStorageKey(configPath), JSON.stringify(prunedLayout));
  }
}

function clearStoredLayout(configPath: string) {
  window.localStorage.removeItem(layoutStorageKey(configPath));
}

function layoutStorageKey(configPath: string) {
  return `${layoutStoragePrefix}${configPath}`;
}

function layoutNodePositions(nodes: GraphNode[], savedLayout: Record<string, LayoutPosition>) {
  const positions = new Map<string, { x: number; y: number }>();

  (["inputs", "pipeline_stages", "outputs"] as GraphLane[]).forEach((lane) => {
    const laneNodes = nodes
      .filter((node) => node.lane === lane)
      .sort((a, b) => a.lane_index - b.lane_index || a.id.localeCompare(b.id));
    let y = laneTop;

    laneNodes.forEach((node) => {
      positions.set(node.id, savedLayout[node.id] ?? { x: laneX[lane], y });
      y += estimatedNodeHeight(node) + nodeGap;
    });
  });

  return positions;
}

function estimatedNodeHeight(node: GraphNode) {
  const channelCount = node.input_channels.length + (node.output_channel ? 1 : 0);
  const channelRows = Math.max(1, Math.ceil(channelCount / 2));
  const nameRows = Math.ceil(node.display_name.length / 26);
  const pathRows = Math.ceil(node.config_path.length / 38);

  return Math.max(168, 104 + channelRows * 34 + nameRows * 22 + pathRows * 18);
}
