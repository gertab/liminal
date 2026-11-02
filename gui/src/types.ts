export type GraphNodeKind = "input" | "pipeline_stage" | "output";
export type GraphLane = "inputs" | "pipeline_stages" | "outputs";
export type DiagnosticSeverity = "warning" | "error";
export type DiagnosticsFilter = "all" | "errors" | "warnings";
export type SaveState = "idle" | "dirty" | "saving" | "error";

export type ResolvedPipelineGraph = {
  schema_version: number;
  summary: GraphSummary;
  nodes: GraphNode[];
  edges: GraphEdge[];
  channels: GraphChannel[];
  diagnostics: GraphDiagnostic[];
};

export type DraftEditResult = {
  graph: ResolvedPipelineGraph;
  content: string;
};

export type GraphSummary = {
  node_count: number;
  edge_count: number;
  channel_count: number;
  diagnostic_count: number;
  error_count: number;
  warning_count: number;
  has_errors: boolean;
};

export type GraphNode = {
  id: string;
  kind: GraphNodeKind;
  lane: GraphLane;
  lane_index: number;
  display_name: string;
  config_path: string;
  pipeline_name: string | null;
  processor_type: string;
  input_channels: string[];
  output_channel: string | null;
  parameters: GraphParameter[];
  timing: GraphParameter[];
  concurrency_type: string;
};

export type GraphParameter = {
  key: string;
  value: string;
  raw_value: JsonValue;
  value_kind: string;
  editable: boolean;
};

export type GraphEdge = {
  id: string;
  source_node_id: string;
  target_node_id: string;
  channel_name: string;
  target_input_index: number;
};

export type GraphChannel = {
  name: string;
  producer_node_ids: string[];
  consumer_node_ids: string[];
  channel_type: string;
  capacity: number;
};

export type GraphDiagnostic = {
  kind: string;
  severity: DiagnosticSeverity;
  message: string;
  channel_name: string | null;
  node_ids: string[];
};

export type ProcessorCategory = "input" | "transform" | "aggregator" | "output";
export type JsonValue = null | string | number | boolean | JsonValue[] | { [key: string]: JsonValue };
export type FieldKind = "string" | "integer" | "number" | "boolean" | "enum" | "array" | "object" | "json_value";
export type FieldStatus = "stable" | "experimental" | "reserved";

export type SchemaSpec =
  | { kind: "object"; fields: FieldSpec[] }
  | { kind: "array"; item: SchemaSpec }
  | { kind: "tagged_union"; tag: string; variants: TaggedVariantSpec[] }
  | { kind: "json_value" };

export type TaggedVariantSpec = {
  tag_value: string;
  label: string;
  fields: FieldSpec[];
};

export type FieldSpec = {
  key: string;
  label: string;
  kind: FieldKind;
  required: boolean;
  default_value: string | null;
  options: string[];
  help: string;
  schema: SchemaSpec | null;
  renderer: string | null;
  status: FieldStatus;
  status_note: string | null;
};

export type ProcessorDescriptor = {
  type_name: string;
  category: ProcessorCategory;
  display_name: string;
  description: string;
  fields: FieldSpec[];
};

export type RuntimeEventKind =
  | "pipeline_starting"
  | "pipeline_started"
  | "pipeline_stopped"
  | "stage_starting"
  | "stage_running"
  | "stage_stopped"
  | "message_received"
  | "message_emitted"
  | "processor_error"
  /** A processor's snapshot for the GUI (in `data`), e.g. the live shopfloor. */
  | "processor_view"
  /** A stage refused a command sent to it (reason in `text`). */
  | "command_rejected";

export type RuntimeEvent = {
  id: number;
  timestamp_ms: number;
  kind: RuntimeEventKind;
  stage_id: string | null;
  processor_type: string | null;
  channel_name: string | null;
  text: string | null;
  data?: unknown;
};

export type RuntimeStageState = "starting" | "running" | "stopped" | "error";

export type RuntimeStageSnapshot = {
  stageId: string;
  processorType: string | null;
  state: RuntimeStageState;
  message: string | null;
  updatedAtMs: number;
};

export type RuntimeStageStates = Record<string, RuntimeStageSnapshot>;

export type RuntimeMessageActivity = {
  stageIds: Record<string, number>;
  channelNames: Record<string, number>;
};

export type RuntimeStageCounters = Record<
  string,
  {
    received: number;
    emitted: number;
    errors: number;
  }
>;

export type RuntimeChannelCounters = Record<
  string,
  {
    received: number;
    emitted: number;
  }
>;
