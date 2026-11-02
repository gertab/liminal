import { invoke } from "@tauri-apps/api/core";
import { ConfigBrowserPanel } from "./components/config-browser/ConfigBrowserPanel";
import {
  connectionValidationMessage,
  GraphCanvas,
} from "./components/graph/GraphCanvas";
import {
  KeyValue,
} from "./components/inspector/InspectorPrimitives";
import { InspectorPanel } from "./components/inspector/InspectorPanel";
import { ShopfloorProvider } from "./components/shopfloor/ShopfloorContext";
import { useRuntimeLogs } from "./hooks/useRuntimeLogs";
import {
  ReactFlowProvider,
} from "@xyflow/react";
import {
  AlertCircle,
  AlertTriangle,
  Boxes,
  GitBranch,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Plus,
  RotateCcw,
  Search,
  Trash2,
} from "lucide-react";
import {
  MouseEvent as ReactMouseEvent,
  ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  DiagnosticsFilter,
  DiagnosticSeverity,
  DraftEditResult,
  GraphDiagnostic,
  GraphEdge,
  GraphNode,
  JsonValue,
  ProcessorCategory,
  ProcessorDescriptor,
  ResolvedPipelineGraph,
  SaveState,
} from "./types";

type DeleteImpact = {
  outputChannel: string | null;
  downstreamNodes: GraphNode[];
};

type ViewportSize = {
  width: number;
  height: number;
};

type PendingDelete = {
  node: GraphNode;
  impact: DeleteImpact;
};

type PendingDiscardAction = {
  title: string;
  detail: string;
  confirmLabel: string;
  run: () => void | Promise<void>;
};

const initialConfigPath = "config/examples/config_rule_filter.toml";
const defaultInspectorWidth = 520;
const minInspectorWidth = 330;
const maxInspectorWidth = 980;
const minGraphColumnWidth = 640;
const inspectorWidthStorageKey = "liminal.inspectorWidth";
const recentConfigsStorageKey = "liminal.recentConfigs";
const workspacePathStorageKey = "liminal.workspacePath";
const showExamplesStorageKey = "liminal.showExamples";
const fileSidebarWidthStorageKey = "liminal.fileSidebarWidth";
const toolsSidebarWidthStorageKey = "liminal.toolsSidebarWidth";
const fileSidebarCollapsedStorageKey = "liminal.fileSidebarCollapsed";
const toolsSidebarCollapsedStorageKey = "liminal.toolsSidebarCollapsed";
const inspectorCollapsedStorageKey = "liminal.inspectorCollapsed";
const maxRecentConfigs = 6;
const defaultFileSidebarWidth = 320;
const defaultToolsSidebarWidth = 330;
const minFileSidebarWidth = 260;
const maxFileSidebarWidth = 520;
const minToolsSidebarWidth = 280;
const maxToolsSidebarWidth = 520;

function App() {
  const [configPath, setConfigPath] = useState(initialConfigPath);
  const [loadedConfigPath, setLoadedConfigPath] = useState<string | null>(null);
  const [exampleConfigs, setExampleConfigs] = useState<string[]>([]);
  const [recentConfigPaths, setRecentConfigPaths] = useState<string[]>(readStoredRecentConfigs);
  const [workspacePath, setWorkspacePath] = useState(readStoredWorkspacePath);
  const [workspaceConfigs, setWorkspaceConfigs] = useState<string[]>([]);
  const [showExamples, setShowExamples] = useState(readStoredShowExamples);
  const [processorDescriptors, setProcessorDescriptors] = useState<ProcessorDescriptor[]>([]);
  const [graph, setGraph] = useState<ResolvedPipelineGraph | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedChannelName, setSelectedChannelName] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [selectedDiagnosticKey, setSelectedDiagnosticKey] = useState<string | null>(null);
  const [diagnosticsFilter, setDiagnosticsFilter] = useState<DiagnosticsFilter>("all");
  const [filterText, setFilterText] = useState("");
  const [loadState, setLoadState] = useState<"idle" | "loading" | "error">("idle");
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [savedContent, setSavedContent] = useState<string | null>(null);
  const [draftContent, setDraftContent] = useState<string | null>(null);
  const [fileSidebarWidthSetting, setFileSidebarWidthSetting] = useState(() =>
    readStoredSidebarWidth(fileSidebarWidthStorageKey, defaultFileSidebarWidth, minFileSidebarWidth, maxFileSidebarWidth),
  );
  const [toolsSidebarWidthSetting, setToolsSidebarWidthSetting] = useState(() =>
    readStoredSidebarWidth(
      toolsSidebarWidthStorageKey,
      defaultToolsSidebarWidth,
      minToolsSidebarWidth,
      maxToolsSidebarWidth,
    ),
  );
  const [inspectorWidth, setInspectorWidth] = useState(readStoredInspectorWidth);
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(null);
  const [pendingDiscardAction, setPendingDiscardAction] = useState<PendingDiscardAction | null>(null);
  const [fileSidebarCollapsed, setFileSidebarCollapsed] = useState(() =>
    readStoredCollapsedState(fileSidebarCollapsedStorageKey),
  );
  const [toolsSidebarCollapsed, setToolsSidebarCollapsed] = useState(() =>
    readStoredCollapsedState(toolsSidebarCollapsedStorageKey),
  );
  const [inspectorCollapsed, setInspectorCollapsed] = useState(() =>
    readStoredCollapsedState(inspectorCollapsedStorageKey),
  );
  const [compactDensity, setCompactDensity] = useState(isCompactDensityViewport);
  const [viewportSize, setViewportSize] = useState(readViewportSize);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const isDirty = savedContent !== null && draftContent !== null && savedContent !== draftContent;
  const hasUnsavedDraft = saveState === "dirty" || isDirty;
  const {
    runtimeState,
    runtimeLogs,
    runtimeEvents,
    runtimeStageStates,
    runtimeStageCounters,
    runtimeChannelCounters,
    runtimeMessageActivity,
    runtimeLastMessageActivity,
    runtimeProcessorViews,
    runtimeLogFilter,
    runtimeContentFilter,
    setRuntimeLogFilter,
    setRuntimeContentFilter,
    startRuntime,
    stopRuntime,
    clearRuntimeLogs,
  } = useRuntimeLogs({
    configPath,
    hasUnsavedDraft,
    onError: setError,
  });

  const applyDraftEdit = useCallback(
    (edit: DraftEditResult, selectedNodeId: string | null, selectedEdgeId: string | null = null) => {
      setGraph(edit.graph);
      setDraftContent(edit.content);
      setSelectedNodeId(selectedNodeId);
      setSelectedChannelName(null);
      setSelectedEdgeId(selectedEdgeId);
      setSelectedDiagnosticKey(null);
      setSaveState("dirty");
    },
    [],
  );

  const loadGraph = useCallback(async (path: string) => {
    setLoadState("loading");
    setError(null);

    try {
      const nextContent = await invoke<string>("load_config_text", { path });
      const nextGraph = await invoke<ResolvedPipelineGraph>("load_graph", { path });
      setConfigPath(path);
      setLoadedConfigPath(path);
      setGraph(nextGraph);
      setSavedContent(nextContent);
      setDraftContent(nextContent);
      setSelectedNodeId(nextGraph.nodes[0]?.id ?? null);
      setSelectedChannelName(null);
      setSelectedEdgeId(null);
      setSelectedDiagnosticKey(null);
      setLoadState("idle");
      setSaveState("idle");
      setRecentConfigPaths(writeStoredRecentConfig(path));
    } catch (caught) {
      setGraph(null);
      setLoadedConfigPath(null);
      setSavedContent(null);
      setDraftContent(null);
      setSelectedNodeId(null);
      setSelectedChannelName(null);
      setSelectedEdgeId(null);
      setSelectedDiagnosticKey(null);
      setError(String(caught));
      setLoadState("error");
      setSaveState("idle");
    }
  }, []);

  const refreshWorkspaceConfigs = useCallback(async (path: string) => {
    if (!path) {
      setWorkspaceConfigs([]);
      return;
    }

    try {
      setWorkspaceConfigs(await invoke<string[]>("list_workspace_configs", { path }));
    } catch (caught) {
      setWorkspaceConfigs([]);
      setError(String(caught));
    }
  }, []);

  const requestDiscardOrRun = useCallback(
    (action: PendingDiscardAction) => {
      if (!hasUnsavedDraft) {
        void action.run();
        return;
      }

      setPendingDiscardAction(action);
    },
    [hasUnsavedDraft],
  );

  const openConfigPath = useCallback(
    (path: string) => {
      requestDiscardOrRun({
        title: "Open Config",
        detail: `Discard the current unsaved draft and open ${path}?`,
        confirmLabel: "Discard and Open",
        run: () => {
          setConfigPath(path);
          loadGraph(path);
        },
      });
    },
    [loadGraph, requestDiscardOrRun],
  );

  const chooseConfigFile = useCallback(async () => {
    let selectedPath: string | null;
    try {
      selectedPath = await invoke<string | null>("pick_config_file");
    } catch (caught) {
      setError(String(caught));
      setLoadState("error");
      return;
    }

    if (typeof selectedPath !== "string") {
      return;
    }

    setConfigPath(selectedPath);
    loadGraph(selectedPath);
  }, [loadGraph]);

  const openConfigFile = useCallback(() => {
    requestDiscardOrRun({
      title: "Open File",
      detail: "Discard the current unsaved draft and choose another TOML config?",
      confirmLabel: "Discard and Choose",
      run: chooseConfigFile,
    });
  }, [chooseConfigFile, requestDiscardOrRun]);

  const openWorkspaceFolder = useCallback(async () => {
    let selectedPath: string | null;
    try {
      selectedPath = await invoke<string | null>("pick_workspace_folder");
    } catch (caught) {
      setError(String(caught));
      setLoadState("error");
      return;
    }

    if (typeof selectedPath !== "string") {
      return;
    }

    setWorkspacePath(selectedPath);
    window.localStorage.setItem(workspacePathStorageKey, selectedPath);
    await refreshWorkspaceConfigs(selectedPath);
  }, [refreshWorkspaceConfigs]);

  const reloadConfig = useCallback(() => {
    requestDiscardOrRun({
      title: "Reload Config",
      detail: `Discard the current unsaved draft and reload ${configPath}?`,
      confirmLabel: "Discard and Reload",
      run: () => loadGraph(configPath),
    });
  }, [configPath, loadGraph, requestDiscardOrRun]);

  const copyIntoWorkspace = useCallback(async (sourcePath = configPath) => {
    if (!workspacePath) {
      setError("Choose a workspace folder before copying a config into it.");
      setSaveState("error");
      return;
    }

    setSaveState("saving");
    setError(null);

    try {
      const content =
        normalizeComparablePath(sourcePath) === normalizeComparablePath(configPath) && draftContent !== null
          ? draftContent
          : await invoke<string>("load_config_text", { path: sourcePath });
      const copiedPath = await invoke<string>("copy_config_to_workspace", {
        workspacePath,
        sourcePath,
        content,
      });
      const nextGraph = await invoke<ResolvedPipelineGraph>("load_graph", { path: copiedPath });
      setGraph(nextGraph);
      setConfigPath(copiedPath);
      setLoadedConfigPath(copiedPath);
      setDraftContent(content);
      setSavedContent(content);
      setRecentConfigPaths(writeStoredRecentConfig(copiedPath));
      setSelectedNodeId(nextGraph.nodes[0]?.id ?? null);
      setSelectedChannelName(null);
      setSelectedEdgeId(null);
      setSelectedDiagnosticKey(null);
      setSaveState("idle");
      await refreshWorkspaceConfigs(workspacePath);
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
    }
  }, [configPath, draftContent, refreshWorkspaceConfigs, workspacePath]);

  const saveDraftAs = useCallback(async () => {
    if (draftContent === null) {
      setError("No editable draft is loaded.");
      setSaveState("error");
      return;
    }

    let selectedPath: string | null;
    try {
      selectedPath = await invoke<string | null>("pick_save_config_path", {
        defaultPath: configPath.endsWith(".toml") ? configPath : "config.toml",
      });
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
      return;
    }

    if (typeof selectedPath !== "string") {
      return;
    }

    setSaveState("saving");
    setError(null);

    try {
      const nextGraph = await invoke<ResolvedPipelineGraph>("save_config_as", {
        path: selectedPath,
        content: draftContent,
      });
      setGraph(nextGraph);
      setConfigPath(selectedPath);
      setLoadedConfigPath(selectedPath);
      setSavedContent(draftContent);
      setRecentConfigPaths(writeStoredRecentConfig(selectedPath));
      setSelectedNodeId(nextGraph.nodes[0]?.id ?? null);
      setSelectedChannelName(null);
      setSelectedEdgeId(null);
      setSelectedDiagnosticKey(null);
      setSaveState("idle");
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
    }
  }, [configPath, draftContent]);

  const createNewConfigFile = useCallback(async () => {
    let selectedPath: string | null;
    try {
      selectedPath = await invoke<string | null>("pick_save_config_path", {
        defaultPath: workspacePath ? `${workspacePath}/config.toml` : "config.toml",
      });
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
      return;
    }

    if (typeof selectedPath !== "string") {
      return;
    }

    setSaveState("saving");
    setError(null);

    try {
      const emptyContent = "";
      const nextGraph = await invoke<ResolvedPipelineGraph>("save_config_as", {
        path: selectedPath,
        content: emptyContent,
      });
      setGraph(nextGraph);
      setConfigPath(selectedPath);
      setLoadedConfigPath(selectedPath);
      setSavedContent(emptyContent);
      setDraftContent(emptyContent);
      setRecentConfigPaths(writeStoredRecentConfig(selectedPath));
      setSelectedNodeId(nextGraph.nodes[0]?.id ?? null);
      setSelectedChannelName(null);
      setSelectedEdgeId(null);
      setSelectedDiagnosticKey(null);
      setSaveState("idle");
      await refreshWorkspaceConfigs(workspacePath);
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
    }
  }, [refreshWorkspaceConfigs, workspacePath]);

  const newConfigFile = useCallback(() => {
    requestDiscardOrRun({
      title: "New Config",
      detail: "Discard the current unsaved draft and create a new empty TOML config?",
      confirmLabel: "Discard and Create",
      run: createNewConfigFile,
    });
  }, [createNewConfigFile, requestDiscardOrRun]);

  useEffect(() => {
    invoke<string[]>("list_example_configs")
      .then(setExampleConfigs)
      .catch(() => setExampleConfigs([]));
    invoke<ProcessorDescriptor[]>("list_processor_descriptors")
      .then(setProcessorDescriptors)
      .catch(() => setProcessorDescriptors([]));
    loadGraph(initialConfigPath);
  }, [loadGraph]);

  useEffect(() => {
    void refreshWorkspaceConfigs(workspacePath);
  }, [refreshWorkspaceConfigs, workspacePath]);

  const selectedRuntimeNode =
    selectedNodeId && graph ? graph.nodes.find((node) => node.id === selectedNodeId) ?? null : null;
  const selectNode = useCallback((id: string | null) => {
    setSelectedNodeId(id);
    setSelectedChannelName(null);
    setSelectedEdgeId(null);
    setSelectedDiagnosticKey(null);
  }, []);
  const selectChannel = useCallback((channelName: string | null) => {
    setSelectedChannelName(channelName);
    setSelectedEdgeId(null);
    if (channelName) {
      setSelectedNodeId(null);
    }
    setSelectedDiagnosticKey(null);
  }, []);
  const selectEdge = useCallback((edgeId: string | null) => {
    setSelectedEdgeId(edgeId);
    if (edgeId) {
      setSelectedNodeId(null);
      setSelectedChannelName(null);
    }
    setSelectedDiagnosticKey(null);
  }, []);
  const selectDiagnostic = useCallback((diagnostic: GraphDiagnostic, index: number) => {
    setSelectedDiagnosticKey(diagnosticKey(diagnostic, index));
    setSelectedEdgeId(null);

    if (diagnostic.channel_name) {
      setSelectedChannelName(diagnostic.channel_name);
      setSelectedNodeId(null);
      return;
    }

    setSelectedNodeId(diagnostic.node_ids[0] ?? null);
    setSelectedChannelName(null);
  }, []);
  const selectDiagnosticByStep = useCallback(
    (step: 1 | -1) => {
      const diagnostics = graph?.diagnostics ?? [];
      if (diagnostics.length === 0) {
        return;
      }

      const currentIndex = diagnostics.findIndex(
        (diagnostic, index) => diagnosticKey(diagnostic, index) === selectedDiagnosticKey,
      );
      const nextIndex =
        currentIndex === -1
          ? step === 1
            ? 0
            : diagnostics.length - 1
          : (currentIndex + step + diagnostics.length) % diagnostics.length;

      selectDiagnostic(diagnostics[nextIndex], nextIndex);
    },
    [graph, selectDiagnostic, selectedDiagnosticKey],
  );
  const updateNodeParameter = useCallback(
    async (nodeId: string, parameterKey: string, value: string) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("update_node_parameter_draft", {
          content: draftContent,
          nodeId,
          parameterKey,
          value,
        });
        applyDraftEdit(edit, nodeId);
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent],
  );
  const updateNodeParameterJson = useCallback(
    async (nodeId: string, parameterKey: string, value: JsonValue) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("update_node_parameter_json_draft", {
          content: draftContent,
          nodeId,
          parameterKey,
          valueJson: JSON.stringify(value),
        });
        applyDraftEdit(edit, nodeId);
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent],
  );
  const connectGraphNodes = useCallback(
    async (sourceNodeId: string, targetNodeId: string) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      const validationMessage = connectionValidationMessage(graph, sourceNodeId, targetNodeId);
      if (validationMessage) {
        setError(validationMessage);
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("connect_nodes_draft", {
          content: draftContent,
          sourceNodeId,
          targetNodeId,
        });
        applyDraftEdit(edit, targetNodeId);
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent, graph],
  );
  const disconnectGraphEdge = useCallback(
    async (targetNodeId: string, channelName: string) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("disconnect_edge_draft", {
          content: draftContent,
          targetNodeId,
          channelName,
        });
        applyDraftEdit(edit, targetNodeId);
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent],
  );
  const addGraphNode = useCallback(
    async (
      processorType: string,
      nodeName: string,
      processorCategory: ProcessorCategory,
      pipelineName: string | null,
    ) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("add_node_draft", {
          content: draftContent,
          processorType,
          nodeName,
          pipelineName,
        });
        applyDraftEdit(edit, nodeIdForNewNode(processorCategory, nodeName, pipelineName));
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent],
  );
  const deleteGraphNode = useCallback(
    async (nodeId: string) => {
      const node = graph?.nodes.find((candidate) => candidate.id === nodeId) ?? null;
      if (!graph || !node) {
        setError(`Node '${nodeId}' was not found.`);
        setSaveState("error");
        return;
      }

      const impact = deletionImpact(graph, node);
      setPendingDelete({ node, impact });
    },
    [graph],
  );
  const confirmDeleteGraphNode = useCallback(async () => {
    if (!pendingDelete) {
      return;
    }

    if (draftContent === null) {
      setError("No editable draft is loaded.");
      setSaveState("error");
      setPendingDelete(null);
      return;
    }

    const nodeId = pendingDelete.node.id;
    setSaveState("saving");
    setError(null);
    setPendingDelete(null);

    try {
      const edit = await invoke<DraftEditResult>("delete_node_draft", {
        content: draftContent,
        nodeId,
      });
      applyDraftEdit(edit, edit.graph.nodes[0]?.id ?? null);
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
    }
  }, [applyDraftEdit, draftContent, pendingDelete]);
  const updateNodeField = useCallback(
    async (nodeId: string, fieldKey: string, value: string) => {
      if (draftContent === null) {
        setError("No editable draft is loaded.");
        setSaveState("error");
        return;
      }

      setSaveState("saving");
      setError(null);

      try {
        const edit = await invoke<DraftEditResult>("update_node_field_draft", {
          content: draftContent,
          nodeId,
          fieldKey,
          value,
        });
        applyDraftEdit(edit, nodeId);
      } catch (caught) {
        setError(String(caught));
        setSaveState("error");
      }
    },
    [applyDraftEdit, draftContent],
  );
  const saveDraft = useCallback(async () => {
    if (draftContent === null || !hasUnsavedDraft) {
      return;
    }

    setSaveState("saving");
    setError(null);

    try {
      const nextGraph = await invoke<ResolvedPipelineGraph>("save_config_text", {
        path: configPath,
        content: draftContent,
      });
      setGraph(nextGraph);
      setSavedContent(draftContent);
      setSaveState("idle");
    } catch (caught) {
      setError(String(caught));
      setSaveState("error");
    }
  }, [configPath, draftContent, hasUnsavedDraft]);
  const revertDraft = useCallback(() => {
    if (!savedContent) {
      return;
    }

    loadGraph(configPath);
  }, [configPath, loadGraph, savedContent]);

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      const key = event.key.toLowerCase();
      const hasPrimaryModifier = event.ctrlKey || event.metaKey;

      if (hasPrimaryModifier && key === "s") {
        event.preventDefault();
        if (event.shiftKey) {
          saveDraftAs();
        } else {
          saveDraft();
        }
        return;
      }

      if (hasPrimaryModifier && key === "o") {
        event.preventDefault();
        if (event.shiftKey) {
          openWorkspaceFolder();
        } else {
          openConfigFile();
        }
        return;
      }

      if (hasPrimaryModifier && key === "n") {
        event.preventDefault();
        newConfigFile();
        return;
      }

      if (hasPrimaryModifier && key === "r") {
        event.preventDefault();
        if (event.shiftKey) {
          revertDraft();
        } else {
          reloadConfig();
        }
        return;
      }

      if (hasPrimaryModifier && key === "f") {
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }

      if (key === "escape" && !isTextEditingTarget(event.target)) {
        if (pendingDelete) {
          setPendingDelete(null);
        } else {
          setSelectedNodeId(null);
          setSelectedChannelName(null);
          setSelectedEdgeId(null);
          setSelectedDiagnosticKey(null);
        }
      }
    };

    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, [
    configPath,
    newConfigFile,
    openConfigFile,
    openWorkspaceFolder,
    pendingDelete,
    reloadConfig,
    revertDraft,
    saveDraft,
    saveDraftAs,
  ]);
  const startInspectorResize = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      event.preventDefault();

      const startX = event.clientX;
      const startWidth = inspectorWidth;

      document.body.classList.add("resizing-inspector");

      const resize = (moveEvent: MouseEvent) => {
        setInspectorWidth(clampInspectorWidth(startWidth + startX - moveEvent.clientX));
      };
      const stopResize = () => {
        document.body.classList.remove("resizing-inspector");
        window.removeEventListener("mousemove", resize);
        window.removeEventListener("mouseup", stopResize);
      };

      window.addEventListener("mousemove", resize);
      window.addEventListener("mouseup", stopResize);
    },
    [inspectorWidth],
  );

  const startFileSidebarResize = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      event.preventDefault();

      const startX = event.clientX;
      const startWidth = fileSidebarWidthSetting;

      document.body.classList.add("resizing-sidebar");

      const resize = (moveEvent: MouseEvent) => {
        setFileSidebarWidthSetting(
          clampSidebarWidth(
            startWidth + moveEvent.clientX - startX,
            minFileSidebarWidth,
            maxFileSidebarWidth,
          ),
        );
      };
      const stopResize = () => {
        document.body.classList.remove("resizing-sidebar");
        window.removeEventListener("mousemove", resize);
        window.removeEventListener("mouseup", stopResize);
      };

      window.addEventListener("mousemove", resize);
      window.addEventListener("mouseup", stopResize);
    },
    [fileSidebarWidthSetting],
  );

  const startToolsSidebarResize = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>) => {
      event.preventDefault();

      const startX = event.clientX;
      const startWidth = toolsSidebarWidthSetting;

      document.body.classList.add("resizing-sidebar");

      const resize = (moveEvent: MouseEvent) => {
        setToolsSidebarWidthSetting(
          clampSidebarWidth(
            startWidth + moveEvent.clientX - startX,
            minToolsSidebarWidth,
            maxToolsSidebarWidth,
          ),
        );
      };
      const stopResize = () => {
        document.body.classList.remove("resizing-sidebar");
        window.removeEventListener("mousemove", resize);
        window.removeEventListener("mouseup", stopResize);
      };

      window.addEventListener("mousemove", resize);
      window.addEventListener("mouseup", stopResize);
    },
    [toolsSidebarWidthSetting],
  );

  useEffect(() => {
    window.localStorage.setItem(inspectorWidthStorageKey, String(inspectorWidth));
  }, [inspectorWidth]);

  useEffect(() => {
    window.localStorage.setItem(fileSidebarWidthStorageKey, String(fileSidebarWidthSetting));
  }, [fileSidebarWidthSetting]);

  useEffect(() => {
    window.localStorage.setItem(toolsSidebarWidthStorageKey, String(toolsSidebarWidthSetting));
  }, [toolsSidebarWidthSetting]);

  useEffect(() => {
    window.localStorage.setItem(showExamplesStorageKey, String(showExamples));
  }, [showExamples]);

  useEffect(() => {
    window.localStorage.setItem(fileSidebarCollapsedStorageKey, String(fileSidebarCollapsed));
  }, [fileSidebarCollapsed]);

  useEffect(() => {
    window.localStorage.setItem(toolsSidebarCollapsedStorageKey, String(toolsSidebarCollapsed));
  }, [toolsSidebarCollapsed]);

  useEffect(() => {
    window.localStorage.setItem(inspectorCollapsedStorageKey, String(inspectorCollapsed));
  }, [inspectorCollapsed]);

  useEffect(() => {
    const updateViewport = () => {
      setCompactDensity(isCompactDensityViewport());
      setViewportSize(readViewportSize());
    };
    window.addEventListener("resize", updateViewport);
    return () => window.removeEventListener("resize", updateViewport);
  }, []);

  const fileSidebarWidth = fileSidebarCollapsed ? 48 : fileSidebarWidthSetting;
  const toolsSidebarWidth = toolsSidebarCollapsed ? 48 : toolsSidebarWidthSetting;
  const leftResizerWidth = fileSidebarCollapsed ? 0 : 8;
  const toolsResizerWidth = toolsSidebarCollapsed ? 0 : 8;
  const resizerWidth = inspectorCollapsed ? 0 : 8;
  const maxEffectiveInspectorWidth = Math.max(
    minInspectorWidth,
    viewportSize.width -
      fileSidebarWidth -
      leftResizerWidth -
      toolsSidebarWidth -
      toolsResizerWidth -
      resizerWidth -
      minGraphColumnWidth,
  );
  const effectiveInspectorWidth = inspectorCollapsed
    ? 48
    : Math.min(inspectorWidth, maxEffectiveInspectorWidth);

  return (
    <ReactFlowProvider>
      <ShopfloorProvider
        graph={graph}
        processorViews={runtimeProcessorViews}
        runtimeState={runtimeState}
        onError={setError}
      >
      <div
        className={compactDensity ? "app-shell compact-density" : "app-shell"}
        style={{
          gridTemplateColumns: [
            `${fileSidebarWidth}px`,
            `${leftResizerWidth}px`,
            `${toolsSidebarWidth}px`,
            `${toolsResizerWidth}px`,
            "minmax(0, 1fr)",
            `${resizerWidth}px`,
            `${effectiveInspectorWidth}px`,
          ].join(" "),
        }}
      >
        <aside className={fileSidebarCollapsed ? "sidebar file-sidebar collapsed" : "sidebar file-sidebar"}>
          {fileSidebarCollapsed ? (
            <button
              className="sidebar-toggle collapsed-toggle"
              onClick={() => setFileSidebarCollapsed(false)}
              aria-label="Expand file sidebar"
              title="Expand file sidebar"
            >
              <PanelLeftOpen size={18} />
            </button>
          ) : (
            <>
              <div className="brand-row">
                <div className="brand-block">
                  <div className="brand-mark">
                    <GitBranch size={21} />
                  </div>
                  <div>
                    <h1>Liminal</h1>
                    <p>Pipeline graph</p>
                  </div>
                </div>
                <button
                  className="sidebar-toggle"
                  onClick={() => setFileSidebarCollapsed(true)}
                  aria-label="Collapse file sidebar"
                  title="Collapse file sidebar"
                >
                  <PanelLeftClose size={17} />
                </button>
              </div>

              <ConfigBrowserPanel
                configPath={configPath}
                draftContent={draftContent}
                exampleConfigs={exampleConfigs}
                isDirty={hasUnsavedDraft}
                loadState={loadState}
                recentConfigPaths={recentConfigPaths}
                saveState={saveState}
                showExamples={showExamples}
                workspacePath={workspacePath}
                workspaceConfigs={workspaceConfigs}
                onClearRecent={() => setRecentConfigPaths(clearStoredRecentConfigs())}
                onCopyIntoWorkspace={copyIntoWorkspace}
                onLoadConfig={openConfigPath}
                onNewConfig={newConfigFile}
                onOpenFile={openConfigFile}
                onOpenFolder={openWorkspaceFolder}
                onReload={reloadConfig}
                onRevert={revertDraft}
                onSave={saveDraft}
                onSaveAs={saveDraftAs}
                onToggleExamples={setShowExamples}
              />
            </>
          )}
        </aside>

        <div
          className={fileSidebarCollapsed ? "sidebar-resizer collapsed" : "sidebar-resizer"}
          onMouseDown={startFileSidebarResize}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize file sidebar"
        />

        <aside className={toolsSidebarCollapsed ? "sidebar tools-sidebar collapsed" : "sidebar tools-sidebar"}>
          {toolsSidebarCollapsed ? (
            <button
              className="sidebar-toggle collapsed-toggle"
              onClick={() => setToolsSidebarCollapsed(false)}
              aria-label="Expand tools sidebar"
              title="Expand tools sidebar"
            >
              <PanelLeftOpen size={18} />
            </button>
          ) : (
            <>
              <div className="tools-sidebar-header">
                <div className="search-row">
                  <Search size={16} />
                  <input
                    ref={searchInputRef}
                    value={filterText}
                    onChange={(event) => setFilterText(event.target.value)}
                    placeholder="Search nodes or channels"
                    aria-label="Search nodes or channels"
                  />
                </div>
                <button
                  className="sidebar-toggle"
                  onClick={() => setToolsSidebarCollapsed(true)}
                  aria-label="Collapse tools sidebar"
                  title="Collapse tools sidebar"
                >
                  <PanelLeftClose size={17} />
                </button>
              </div>

              <ToolsSidebarSection title="Add Node" icon={<Plus size={16} />}>
                <AddNodePanel
                  graph={graph}
                  hideTitle
                  processorDescriptors={processorDescriptors}
                  saveState={saveState}
                  onAddNode={addGraphNode}
                />
              </ToolsSidebarSection>

              <ToolsSidebarSection title="Graph Status" icon={<Boxes size={16} />}>
                <SummaryPanel graph={graph} hideTitle />
                <GraphStatusPanel
                  graph={graph}
                  loadState={loadState}
                  saveState={saveState}
                  error={error}
                  isDirty={hasUnsavedDraft}
                />
              </ToolsSidebarSection>

              <ToolsSidebarSection title="Diagnostics" icon={<AlertCircle size={16} />} grow>
                <DiagnosticsPanel
                  graph={graph}
                  hideTitle
                  filter={diagnosticsFilter}
                  onFilterChange={setDiagnosticsFilter}
                  selectedDiagnosticKey={selectedDiagnosticKey}
                  onSelectDiagnostic={selectDiagnostic}
                  onPreviousDiagnostic={() => selectDiagnosticByStep(-1)}
                  onNextDiagnostic={() => selectDiagnosticByStep(1)}
                />
              </ToolsSidebarSection>
            </>
          )}
        </aside>

        <div
          className={toolsSidebarCollapsed ? "sidebar-resizer collapsed" : "sidebar-resizer"}
          onMouseDown={startToolsSidebarResize}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize tools sidebar"
        />

        <main className="workspace">
          <GraphCanvas
            graph={graph}
            configPath={loadedConfigPath ?? configPath}
            selectedNodeId={selectedNodeId}
            selectedChannelName={selectedChannelName}
            selectedEdgeId={selectedEdgeId}
            selectedDiagnosticKey={selectedDiagnosticKey}
            filterText={filterText}
            onSelectNode={selectNode}
            onSelectChannel={selectChannel}
            onSelectEdge={selectEdge}
            onConnectNodes={connectGraphNodes}
            onDisconnectEdge={disconnectGraphEdge}
            onDeleteNode={deleteGraphNode}
            onStartRuntime={startRuntime}
            onStopRuntime={stopRuntime}
            error={error}
            loadState={loadState}
            runtimeState={runtimeState}
            runtimeLogs={runtimeLogs}
            runtimeEvents={runtimeEvents}
            runtimeStageStates={runtimeStageStates}
            runtimeMessageActivity={runtimeMessageActivity}
            runtimeLogFilter={runtimeLogFilter}
            runtimeContentFilter={runtimeContentFilter}
            selectedRuntimeNode={selectedRuntimeNode}
            selectedRuntimeChannelName={selectedChannelName}
            onRuntimeLogFilterChange={setRuntimeLogFilter}
            onRuntimeContentFilterChange={setRuntimeContentFilter}
            onClearRuntimeLogs={clearRuntimeLogs}
          />
        </main>

        <div
          className={inspectorCollapsed ? "inspector-resizer collapsed" : "inspector-resizer"}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize inspector"
          title="Resize inspector"
          onMouseDown={inspectorCollapsed ? undefined : startInspectorResize}
          onDoubleClick={() => setInspectorWidth(defaultInspectorWidth)}
        />

        {inspectorCollapsed ? (
          <aside className="inspector collapsed">
            <button
              className="sidebar-toggle collapsed-toggle"
              onClick={() => setInspectorCollapsed(false)}
              aria-label="Expand inspector"
              title="Expand inspector"
            >
              <PanelRightOpen size={18} />
            </button>
          </aside>
        ) : (
          <div className="inspector-shell">
            <button
              className="sidebar-toggle inspector-collapse-button"
              onClick={() => setInspectorCollapsed(true)}
              aria-label="Collapse inspector"
              title="Collapse inspector"
            >
              <PanelRightClose size={17} />
            </button>
            <InspectorPanel
              graph={graph}
              configPath={configPath}
              processorDescriptors={processorDescriptors}
              selectedNodeId={selectedNodeId}
              selectedChannelName={selectedChannelName}
              selectedEdgeId={selectedEdgeId}
              selectedDiagnosticKey={selectedDiagnosticKey}
              runtimeStageStates={runtimeStageStates}
              runtimeStageCounters={runtimeStageCounters}
              runtimeChannelCounters={runtimeChannelCounters}
              runtimeMessageActivity={runtimeLastMessageActivity}
              onSelectNode={selectNode}
              onSelectChannel={selectChannel}
              onSelectEdge={selectEdge}
              onUpdateNodeParameter={updateNodeParameter}
              onUpdateNodeParameterJson={updateNodeParameterJson}
              onUpdateNodeField={updateNodeField}
              onDisconnectEdge={disconnectGraphEdge}
              onDeleteNode={deleteGraphNode}
              saveState={saveState}
            />
          </div>
        )}
        {pendingDelete && (
          <DeleteNodeDialog
            pendingDelete={pendingDelete}
            saveState={saveState}
            onCancel={() => setPendingDelete(null)}
            onConfirm={confirmDeleteGraphNode}
          />
        )}
        {pendingDiscardAction && (
          <UnsavedChangesDialog
            action={pendingDiscardAction}
            onCancel={() => setPendingDiscardAction(null)}
            onConfirm={async () => {
              const action = pendingDiscardAction;
              setPendingDiscardAction(null);
              await action.run();
            }}
          />
        )}
      </div>
      </ShopfloorProvider>
    </ReactFlowProvider>
  );
}

function SummaryPanel({ graph, hideTitle = false }: { graph: ResolvedPipelineGraph | null; hideTitle?: boolean }) {
  const summary = graph?.summary;

  return (
    <section className="panel">
      {!hideTitle && (
        <div className="panel-title">
          <Boxes size={16} />
          <span>Graph</span>
        </div>
      )}
      <div className="metric-grid">
        <Metric label="Nodes" value={summary?.node_count ?? 0} />
        <Metric label="Edges" value={summary?.edge_count ?? 0} />
        <Metric label="Channels" value={summary?.channel_count ?? 0} />
        <Metric label="Errors" value={summary?.error_count ?? 0} />
      </div>
    </section>
  );
}

function GraphStatusPanel({
  graph,
  loadState,
  saveState,
  error,
  isDirty,
}: {
  graph: ResolvedPipelineGraph | null;
  loadState: "idle" | "loading" | "error";
  saveState: SaveState;
  error: string | null;
  isDirty: boolean;
}) {
  const status = graphStatus(graph, loadState, saveState, error, isDirty);

  return (
    <section className={`status-panel ${status.severity}`}>
      <div>
        <span>{status.label}</span>
        <p>{status.detail}</p>
      </div>
    </section>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ToolsSidebarSection({
  title,
  icon,
  grow = false,
  children,
}: {
  title: string;
  icon: ReactNode;
  grow?: boolean;
  children: ReactNode;
}) {
  return (
    <section className={grow ? "tools-sidebar-section grow" : "tools-sidebar-section"}>
      <div className="tools-sidebar-section-title">
        {icon}
        <span>{title}</span>
      </div>
      <div className="tools-sidebar-section-body">{children}</div>
    </section>
  );
}

function AddNodePanel({
  graph,
  hideTitle = false,
  processorDescriptors,
  saveState,
  onAddNode,
}: {
  graph: ResolvedPipelineGraph | null;
  hideTitle?: boolean;
  processorDescriptors: ProcessorDescriptor[];
  saveState: SaveState;
  onAddNode: (
    processorType: string,
    nodeName: string,
    processorCategory: ProcessorCategory,
    pipelineName: string | null,
  ) => Promise<void>;
}) {
  const descriptors = useMemo(
    () =>
      [...processorDescriptors].sort(
        (left, right) =>
          categoryOrder(left.category) - categoryOrder(right.category) ||
          left.display_name.localeCompare(right.display_name),
      ),
    [processorDescriptors],
  );
  const [processorType, setProcessorType] = useState("");
  const [nodeName, setNodeName] = useState("");
  const [pipelineName, setPipelineName] = useState("");
  const selectedDescriptor =
    descriptors.find((descriptor) => descriptor.type_name === processorType) ?? descriptors[0] ?? null;
  const pipelineOptions = useMemo(() => pipelineNames(graph), [graph]);
  const needsPipeline =
    selectedDescriptor?.category === "transform" || selectedDescriptor?.category === "aggregator";
  const normalizedNodeName = nodeName.trim();
  const normalizedPipelineName = pipelineName.trim() || pipelineOptions[0] || "default_pipeline";
  const validationMessage = selectedDescriptor
    ? addNodeValidationMessage(graph, selectedDescriptor.category, normalizedNodeName, normalizedPipelineName)
    : "Processor descriptors are not loaded.";

  useEffect(() => {
    if (!selectedDescriptor) {
      return;
    }

    if (processorType !== selectedDescriptor.type_name) {
      setProcessorType(selectedDescriptor.type_name);
    }
  }, [processorType, selectedDescriptor]);

  useEffect(() => {
    if (selectedDescriptor) {
      setNodeName(defaultNodeName(graph, selectedDescriptor));
    }
  }, [graph, selectedDescriptor?.type_name]);

  useEffect(() => {
    setPipelineName((currentName) => currentName || pipelineOptions[0] || "default_pipeline");
  }, [pipelineOptions]);

  return (
    <section className="panel add-node-panel">
      {!hideTitle && (
        <div className="panel-title">
          <Plus size={16} />
          <span>Add Node</span>
        </div>
      )}
      <form
        className="add-node-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (!selectedDescriptor || validationMessage) {
            return;
          }

          onAddNode(
            selectedDescriptor.type_name,
            normalizedNodeName,
            selectedDescriptor.category,
            needsPipeline ? normalizedPipelineName : null,
          );
        }}
      >
        <label>
          <span>Processor</span>
          <select
            value={selectedDescriptor?.type_name ?? ""}
            onChange={(event) => setProcessorType(event.target.value)}
            disabled={descriptors.length === 0}
          >
            {descriptors.map((descriptor) => (
              <option key={descriptor.type_name} value={descriptor.type_name}>
                {descriptor.display_name}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Name</span>
          <input
            value={nodeName}
            onChange={(event) => setNodeName(event.target.value)}
            placeholder="node_name"
            aria-label="New node name"
          />
        </label>
        {needsPipeline && (
          <label>
            <span>Pipeline</span>
            <input
              value={pipelineName}
              onChange={(event) => setPipelineName(event.target.value)}
              list="pipeline-options"
              placeholder="default_pipeline"
              aria-label="Pipeline name"
            />
            <datalist id="pipeline-options">
              {pipelineOptions.map((name) => (
                <option key={name} value={name} />
              ))}
            </datalist>
          </label>
        )}
        <div className="add-node-footer">
          <span>{selectedDescriptor?.category ?? "processor"}</span>
          <button disabled={saveState === "saving" || Boolean(validationMessage)}>
            {saveState === "saving" ? "Adding" : "Add"}
          </button>
        </div>
        {validationMessage && <p className="form-hint">{validationMessage}</p>}
      </form>
    </section>
  );
}

function DiagnosticsPanel({
  graph,
  hideTitle = false,
  filter,
  onFilterChange,
  selectedDiagnosticKey,
  onSelectDiagnostic,
  onPreviousDiagnostic,
  onNextDiagnostic,
}: {
  graph: ResolvedPipelineGraph | null;
  hideTitle?: boolean;
  filter: DiagnosticsFilter;
  onFilterChange: (filter: DiagnosticsFilter) => void;
  selectedDiagnosticKey: string | null;
  onSelectDiagnostic: (diagnostic: GraphDiagnostic, index: number) => void;
  onPreviousDiagnostic: () => void;
  onNextDiagnostic: () => void;
}) {
  const diagnostics = graph?.diagnostics ?? [];
  const filteredDiagnostics = diagnostics
    .map((diagnostic, index) => ({ diagnostic, index }))
    .filter(({ diagnostic }) => diagnosticMatchesFilter(diagnostic, filter));
  const groupedDiagnostics = groupDiagnosticsBySeverity(filteredDiagnostics);

  return (
    <section className="panel diagnostics-panel">
      {!hideTitle && (
        <div className="panel-title">
          <AlertCircle size={16} />
          <span>Diagnostics</span>
        </div>
      )}
      <div className="diagnostic-toolbar">
        <div className="segmented-control" aria-label="Diagnostics filter">
          <button className={filter === "all" ? "active" : ""} onClick={() => onFilterChange("all")}>
            All
          </button>
          <button className={filter === "errors" ? "active" : ""} onClick={() => onFilterChange("errors")}>
            Errors
          </button>
          <button className={filter === "warnings" ? "active" : ""} onClick={() => onFilterChange("warnings")}>
            Warnings
          </button>
        </div>
        <div className="diagnostic-nav">
          <button onClick={onPreviousDiagnostic} disabled={diagnostics.length === 0} aria-label="Previous diagnostic">
            Prev
          </button>
          <button onClick={onNextDiagnostic} disabled={diagnostics.length === 0} aria-label="Next diagnostic">
            Next
          </button>
        </div>
      </div>
      {diagnostics.length === 0 ? (
        <p className="empty-state">No graph diagnostics.</p>
      ) : filteredDiagnostics.length === 0 ? (
        <p className="empty-state">No diagnostics match this filter.</p>
      ) : (
        <div className="diagnostic-list">
          {groupedDiagnostics.map(({ severity, items }) => (
            <div className="diagnostic-group" key={severity}>
              <div className="diagnostic-group-title">
                <span>{severity}</span>
                <strong>{items.length}</strong>
              </div>
              {items.map(({ diagnostic, index }) => (
                <button
                  key={`${diagnostic.kind}-${diagnostic.channel_name}-${index}`}
                  className={[
                    "diagnostic",
                    diagnostic.severity,
                    diagnosticKey(diagnostic, index) === selectedDiagnosticKey ? "selected" : "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  onClick={() => onSelectDiagnostic(diagnostic, index)}
                >
                  <div className="diagnostic-meta">
                    <span>{diagnostic.kind}</span>
                    <code>{diagnostic.channel_name ?? diagnostic.node_ids[0] ?? "graph"}</code>
                  </div>
                  <p>{diagnostic.message}</p>
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function UnsavedChangesDialog({
  action,
  onCancel,
  onConfirm,
}: {
  action: PendingDiscardAction;
  onCancel: () => void;
  onConfirm: () => Promise<void>;
}) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCancel();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onCancel}>
      <section
        className="confirm-dialog neutral"
        role="dialog"
        aria-modal="true"
        aria-labelledby="unsaved-changes-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="confirm-dialog-header">
          <div className="confirm-dialog-icon">
            <AlertCircle size={20} />
          </div>
          <div>
            <span>Unsaved Draft</span>
            <h2 id="unsaved-changes-title">{action.title}</h2>
          </div>
        </div>

        <div className="confirm-dialog-body">
          <p className="confirm-dialog-message">{action.detail}</p>
        </div>

        <div className="confirm-dialog-actions">
          <button className="secondary-action" onClick={onCancel}>
            Keep Editing
          </button>
          <button className="primary-action" onClick={onConfirm}>
            <RotateCcw size={15} />
            <span>{action.confirmLabel}</span>
          </button>
        </div>
      </section>
    </div>
  );
}

function DeleteNodeDialog({
  pendingDelete,
  saveState,
  onCancel,
  onConfirm,
}: {
  pendingDelete: PendingDelete;
  saveState: SaveState;
  onCancel: () => void;
  onConfirm: () => Promise<void>;
}) {
  const { node, impact } = pendingDelete;
  const isSaving = saveState === "saving";

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !isSaving) {
        onCancel();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isSaving, onCancel]);

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={() => !isSaving && onCancel()}>
      <section
        className="confirm-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-node-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="confirm-dialog-header">
          <div className="confirm-dialog-icon">
            <AlertTriangle size={20} />
          </div>
          <div>
            <span>Delete Node</span>
            <h2 id="delete-node-title">{node.display_name}</h2>
          </div>
        </div>

        <div className="confirm-dialog-body">
          <KeyValue label="Processor" value={node.processor_type} />
          <KeyValue label="Config path" value={node.config_path} />
          {impact.outputChannel ? (
            <div className="delete-impact">
              <span>Output cleanup</span>
              <strong>{impact.outputChannel}</strong>
              {impact.downstreamNodes.length > 0 ? (
                <>
                  <p>This channel will be removed from downstream inputs:</p>
                  <div className="impact-node-list">
                    {impact.downstreamNodes.map((downstreamNode) => (
                      <code key={downstreamNode.id}>{downstreamNode.display_name}</code>
                    ))}
                  </div>
                </>
              ) : (
                <p>No downstream consumers use this output channel.</p>
              )}
            </div>
          ) : (
            <div className="delete-impact">
              <span>Output cleanup</span>
              <p>This node does not produce a channel.</p>
            </div>
          )}
        </div>

        <div className="confirm-dialog-actions">
          <button className="secondary-action" disabled={isSaving} onClick={onCancel}>
            Cancel
          </button>
          <button className="danger-action" disabled={isSaving} onClick={onConfirm}>
            <Trash2 size={15} />
            <span>{isSaving ? "Deleting" : "Delete Node"}</span>
          </button>
        </div>
      </section>
    </div>
  );
}

function graphStatus(
  graph: ResolvedPipelineGraph | null,
  loadState: "idle" | "loading" | "error",
  saveState: SaveState,
  error: string | null,
  isDirty: boolean,
) {
  if (loadState === "loading") {
    return {
      severity: "loading",
      label: "Loading",
      detail: "Parsing config and resolving graph.",
    };
  }

  if (loadState === "error") {
    return {
      severity: "error",
      label: "Invalid Config",
      detail: error ?? "The config could not be loaded.",
    };
  }

  if (saveState === "saving") {
    return {
      severity: "loading",
      label: "Saving",
      detail: "Writing the current draft to disk.",
    };
  }

  if (saveState === "error") {
    return {
      severity: "error",
      label: "Edit Rejected",
      detail: error ?? "The parameter edit could not be saved.",
    };
  }

  if (saveState === "dirty" || isDirty) {
    return {
      severity: "warning",
      label: "Unsaved Draft",
      detail: "Edits are staged in the GUI. Save to write the TOML file.",
    };
  }

  if (!graph) {
    return {
      severity: "neutral",
      label: "No Graph",
      detail: "No resolved graph is available.",
    };
  }

  if (graph.summary.error_count > 0) {
    return {
      severity: "error",
      label: "Errors",
      detail: `${graph.summary.error_count} error diagnostics require attention.`,
    };
  }

  if (graph.summary.warning_count > 0) {
    return {
      severity: "warning",
      label: "Warnings",
      detail: `${graph.summary.warning_count} warning diagnostics found.`,
    };
  }

  return {
    severity: "valid",
    label: "Valid Graph",
    detail: "No graph diagnostics detected.",
  };
}

function diagnosticMatchesFilter(diagnostic: GraphDiagnostic, filter: DiagnosticsFilter) {
  if (filter === "errors") {
    return diagnostic.severity === "error";
  }

  if (filter === "warnings") {
    return diagnostic.severity === "warning";
  }

  return true;
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

function normalizeComparablePath(path: string) {
  return path.trim().replace(/\\/g, "/").replace(/\/+/g, "/").toLowerCase();
}

function groupDiagnosticsBySeverity(items: { diagnostic: GraphDiagnostic; index: number }[]) {
  return (["error", "warning"] as DiagnosticSeverity[])
    .map((severity) => ({
      severity,
      items: items.filter(({ diagnostic }) => diagnostic.severity === severity),
    }))
    .filter((group) => group.items.length > 0);
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

function deletionImpact(graph: ResolvedPipelineGraph, node: GraphNode): DeleteImpact {
  if (!node.output_channel) {
    return { outputChannel: null, downstreamNodes: [] };
  }

  const downstreamNodeIds = new Set(
    graph.edges
      .filter((edge) => edge.source_node_id === node.id && edge.channel_name === node.output_channel)
      .map((edge) => edge.target_node_id),
  );

  return {
    outputChannel: node.output_channel,
    downstreamNodes: graph.nodes.filter((candidate) => downstreamNodeIds.has(candidate.id)),
  };
}

function categoryOrder(category: ProcessorCategory) {
  return category === "input" ? 0 : category === "transform" ? 1 : category === "aggregator" ? 2 : 3;
}

function pipelineNames(graph: ResolvedPipelineGraph | null) {
  const names = new Set<string>();

  graph?.nodes.forEach((node) => {
    if (node.pipeline_name) {
      names.add(node.pipeline_name);
    }
  });

  return [...names].sort();
}

function defaultNodeName(graph: ResolvedPipelineGraph | null, descriptor: ProcessorDescriptor) {
  const baseName = sanitizeNodeName(descriptor.type_name) || "node";
  const existingNames = new Set(graph?.nodes.map((node) => node.display_name) ?? []);

  if (!existingNames.has(baseName)) {
    return baseName;
  }

  for (let index = 2; index < 1000; index += 1) {
    const candidate = `${baseName}_${index}`;
    if (!existingNames.has(candidate)) {
      return candidate;
    }
  }

  return `${baseName}_${Date.now()}`;
}

function sanitizeNodeName(value: string) {
  return value
    .trim()
    .replace(/[^A-Za-z0-9_-]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function addNodeValidationMessage(
  graph: ResolvedPipelineGraph | null,
  category: ProcessorCategory,
  nodeName: string,
  pipelineName: string,
) {
  if (!nodeName) {
    return "Enter a node name.";
  }

  if (!/^[A-Za-z0-9_-]+$/.test(nodeName)) {
    return "Use letters, numbers, underscores, or hyphens.";
  }

  if ((category === "transform" || category === "aggregator") && !pipelineName) {
    return "Enter a pipeline name.";
  }

  if (pipelineName && !/^[A-Za-z0-9_-]+$/.test(pipelineName)) {
    return "Pipeline names use the same characters as node names.";
  }

  if (graph?.nodes.some((node) => node.id === nodeIdForNewNode(category, nodeName, pipelineName))) {
    return "A node with this name already exists.";
  }

  return null;
}

function nodeIdForNewNode(category: ProcessorCategory, nodeName: string, pipelineName: string | null) {
  if (category === "input") {
    return `input:${nodeName}`;
  }

  if (category === "output") {
    return `output:${nodeName}`;
  }

  return `pipeline:${pipelineName || "default_pipeline"}.stage:${nodeName}`;
}

function readStoredInspectorWidth() {
  const storedWidth = window.localStorage.getItem(inspectorWidthStorageKey);
  const parsedWidth = storedWidth ? Number(storedWidth) : defaultInspectorWidth;

  return clampInspectorWidth(Number.isFinite(parsedWidth) ? parsedWidth : defaultInspectorWidth);
}

function clampInspectorWidth(width: number) {
  return Math.min(Math.max(Math.round(width), minInspectorWidth), maxInspectorWidth);
}

function readStoredSidebarWidth(
  storageKey: string,
  fallbackWidth: number,
  minWidth: number,
  maxWidth: number,
) {
  const storedWidth = window.localStorage.getItem(storageKey);
  const parsedWidth = storedWidth ? Number(storedWidth) : fallbackWidth;

  return clampSidebarWidth(Number.isFinite(parsedWidth) ? parsedWidth : fallbackWidth, minWidth, maxWidth);
}

function clampSidebarWidth(width: number, minWidth: number, maxWidth: number) {
  return Math.min(Math.max(Math.round(width), minWidth), maxWidth);
}

function isCompactDensityViewport() {
  const isApplePlatform = /Mac|iPhone|iPad|iPod/.test(window.navigator.platform);
  return isApplePlatform || window.innerHeight <= 920;
}

function readViewportSize(): ViewportSize {
  return {
    width: window.innerWidth,
    height: window.innerHeight,
  };
}

function readStoredRecentConfigs() {
  try {
    const storedRecentConfigs = window.localStorage.getItem(recentConfigsStorageKey);
    if (!storedRecentConfigs) {
      return [];
    }

    const parsedRecentConfigs = JSON.parse(storedRecentConfigs);
    if (!Array.isArray(parsedRecentConfigs)) {
      return [];
    }

    return parsedRecentConfigs
      .filter((path): path is string => typeof path === "string" && path.trim().length > 0)
      .slice(0, maxRecentConfigs);
  } catch {
    return [];
  }
}

function writeStoredRecentConfig(path: string) {
  const normalizedPath = path.trim();
  if (!normalizedPath) {
    return readStoredRecentConfigs();
  }

  const recentConfigs = [
    normalizedPath,
    ...readStoredRecentConfigs().filter((recentPath) => recentPath !== normalizedPath),
  ].slice(0, maxRecentConfigs);

  window.localStorage.setItem(recentConfigsStorageKey, JSON.stringify(recentConfigs));
  return recentConfigs;
}

function clearStoredRecentConfigs() {
  window.localStorage.removeItem(recentConfigsStorageKey);
  return [];
}

function readStoredWorkspacePath() {
  return window.localStorage.getItem(workspacePathStorageKey) ?? "";
}

function readStoredShowExamples() {
  return window.localStorage.getItem(showExamplesStorageKey) !== "false";
}

function readStoredCollapsedState(storageKey: string) {
  return window.localStorage.getItem(storageKey) === "true";
}

export default App;
