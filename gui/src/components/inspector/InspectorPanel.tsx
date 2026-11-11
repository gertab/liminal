import { Info, Network, Trash2 } from "lucide-react";
import { useCallback } from "react";
import {
  ChannelButtonList,
  DiagnosticsList,
  FieldStatusBadge,
  InspectorSection,
  InspectorTitle,
  KeyValue,
  NodeButtonList,
  SelectedDiagnostic,
} from "./InspectorPrimitives";
import {
  DescriptorSummary,
  EditableFieldRow,
  MissingParameterRow,
  ParameterRow,
} from "./ParameterEditors";
import { fieldApplies, inapplicableReason, parameterValues } from "./fieldConditions";
import {
  GraphChannel,
  GraphDiagnostic,
  GraphEdge,
  GraphNode,
  JsonValue,
  ProcessorDescriptor,
  ResolvedPipelineGraph,
  RuntimeChannelCounters,
  RuntimeMessageActivity,
  RuntimeStageCounters,
  RuntimeStageSnapshot,
  RuntimeStageStates,
  SaveState,
} from "../../types";
export function InspectorPanel({
  graph,
  configPath,
  processorDescriptors,
  selectedNodeId,
  selectedChannelName,
  selectedEdgeId,
  selectedDiagnosticKey,
  runtimeStageStates,
  runtimeStageCounters,
  runtimeChannelCounters,
  runtimeMessageActivity,
  onSelectNode,
  onSelectChannel,
  onSelectEdge,
  onUpdateNodeParameter,
  onUpdateNodeParameterJson,
  onRemoveNodeParameter,
  onUpdateNodeField,
  onDisconnectEdge,
  onDeleteNode,
  saveState,
}: {
  graph: ResolvedPipelineGraph | null;
  configPath: string;
  processorDescriptors: ProcessorDescriptor[];
  selectedNodeId: string | null;
  selectedChannelName: string | null;
  selectedEdgeId: string | null;
  selectedDiagnosticKey: string | null;
  runtimeStageStates: RuntimeStageStates;
  runtimeStageCounters: RuntimeStageCounters;
  runtimeChannelCounters: RuntimeChannelCounters;
  runtimeMessageActivity: RuntimeMessageActivity;
  onSelectNode: (id: string | null) => void;
  onSelectChannel: (channelName: string | null) => void;
  onSelectEdge: (edgeId: string | null) => void;
  onUpdateNodeParameter: (nodeId: string, parameterKey: string, value: string) => Promise<void>;
  onUpdateNodeParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
  onRemoveNodeParameter: (nodeId: string, parameterKey: string) => Promise<void>;
  onUpdateNodeField: (nodeId: string, fieldKey: string, value: string) => Promise<void>;
  onDisconnectEdge: (targetNodeId: string, channelName: string) => Promise<void>;
  onDeleteNode: (nodeId: string) => Promise<void>;
  saveState: SaveState;
}) {
  const selectedNode = graph?.nodes.find((node) => node.id === selectedNodeId) ?? null;
  const selectedEdge = graph?.edges.find((edge) => edge.id === selectedEdgeId) ?? null;
  const selectedChannel =
    graph?.channels.find((channel) => channel.name === selectedChannelName) ?? null;
  const nodeDiagnostics = selectedNode
    ? graph?.diagnostics.filter((diagnostic) => diagnostic.node_ids.includes(selectedNode.id)) ?? []
    : [];
  const channelDiagnostics = selectedChannelName
    ? graph?.diagnostics.filter((diagnostic) => diagnostic.channel_name === selectedChannelName) ?? []
    : [];
  const selectedDiagnostic =
    graph?.diagnostics.find((diagnostic, index) => diagnosticKey(diagnostic, index) === selectedDiagnosticKey) ??
    null;
  const selectNode = useCallback(
    (id: string | null) => {
      onSelectNode(id);
      onSelectChannel(null);
    },
    [onSelectChannel, onSelectNode],
  );
  const selectChannel = useCallback(
    (channelName: string | null) => {
      onSelectChannel(channelName);
      if (channelName) {
        onSelectNode(null);
      }
    },
    [onSelectChannel, onSelectNode],
  );
  const disconnectEdge = useCallback(
    async (edge: GraphEdge) => {
      await onDisconnectEdge(edge.target_node_id, edge.channel_name);
      onSelectEdge(null);
    },
    [onDisconnectEdge, onSelectEdge],
  );

  return (
    <aside className="inspector">
      <div className="inspector-header">
        <Info size={17} />
        <span>Inspector</span>
      </div>

      {!graph ? (
        <p className="empty-state">No graph loaded.</p>
      ) : selectedEdge ? (
        <EdgeInspector
          edge={selectedEdge}
          graph={graph}
          saveState={saveState}
          onSelectNode={selectNode}
          onSelectChannel={selectChannel}
          onDisconnectEdge={disconnectEdge}
        />
      ) : selectedChannel ? (
        <ChannelInspector
          channel={selectedChannel}
          graph={graph}
          diagnostics={channelDiagnostics}
          selectedDiagnostic={selectedDiagnostic}
          runtimeMessageActivity={runtimeMessageActivity}
          runtimeChannelCounters={runtimeChannelCounters}
          onSelectNode={selectNode}
        />
      ) : selectedChannelName ? (
        <MissingChannelInspector
          channelName={selectedChannelName}
          diagnostics={channelDiagnostics}
          selectedDiagnostic={selectedDiagnostic}
        />
      ) : selectedNode ? (
        <NodeInspector
          node={selectedNode}
          configPath={configPath}
          graph={graph}
          processorDescriptors={processorDescriptors}
          diagnostics={nodeDiagnostics}
          selectedDiagnostic={selectedDiagnostic}
          runtimeSnapshot={runtimeStageSnapshotForNode(selectedNode, runtimeStageStates)}
          runtimeCounters={runtimeStageCounters[stageNameForNode(selectedNode) ?? ""] ?? null}
          runtimeMessageActivity={runtimeMessageActivity}
          onSelectChannel={selectChannel}
          onUpdateParameter={onUpdateNodeParameter}
          onUpdateParameterJson={onUpdateNodeParameterJson}
          onRemoveParameter={onRemoveNodeParameter}
          onUpdateField={onUpdateNodeField}
          onDeleteNode={onDeleteNode}
          saveState={saveState}
        />
      ) : (
        <div className="inspector-empty">
          <Network size={24} />
          <p>Select a node, channel, or edge.</p>
        </div>
      )}
    </aside>
  );
}

function NodeInspector({
  node,
  configPath,
  graph,
  processorDescriptors,
  diagnostics,
  selectedDiagnostic,
  runtimeSnapshot,
  runtimeCounters,
  runtimeMessageActivity,
  onSelectChannel,
  onUpdateParameter,
  onUpdateParameterJson,
  onRemoveParameter,
  onUpdateField,
  onDeleteNode,
  saveState,
}: {
  node: GraphNode;
  configPath: string;
  graph: ResolvedPipelineGraph;
  processorDescriptors: ProcessorDescriptor[];
  diagnostics: GraphDiagnostic[];
  selectedDiagnostic: GraphDiagnostic | null;
  runtimeSnapshot: RuntimeStageSnapshot | null;
  runtimeCounters: RuntimeStageCounters[string] | null;
  runtimeMessageActivity: RuntimeMessageActivity;
  onSelectChannel: (channelName: string | null) => void;
  onUpdateParameter: (nodeId: string, parameterKey: string, value: string) => Promise<void>;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
  onRemoveParameter: (nodeId: string, parameterKey: string) => Promise<void>;
  onUpdateField: (nodeId: string, fieldKey: string, value: string) => Promise<void>;
  onDeleteNode: (nodeId: string) => Promise<void>;
  saveState: SaveState;
}) {
  const incomingEdges = graph.edges.filter((edge) => edge.target_node_id === node.id);
  const outgoingEdges = graph.edges.filter((edge) => edge.source_node_id === node.id);
  const outputChannel = node.output_channel
    ? graph.channels.find((channel) => channel.name === node.output_channel) ?? null
    : null;
  const processorDescriptor =
    processorDescriptors.find((descriptor) => descriptor.type_name === node.processor_type) ?? null;
  const parameterFields = processorDescriptor?.fields ?? [];
  const nodeValues = parameterValues(node.parameters, parameterFields);
  // Fields that don't apply to this node (e.g. size_n for a tumbling window) aren't offered.
  const applicableFields = parameterFields.filter((field) => fieldApplies(field, nodeValues));
  const knownParameterCount = applicableFields.length;
  const missingParameterFields = applicableFields.filter(
    (field) => !node.parameters.some((parameter) => parameter.key === field.key),
  );

  return (
    <div className="inspector-body">
      <InspectorTitle eyebrow={node.processor_type} title={node.display_name} />
      <div className="edge-actions">
        <button
          className="danger-action"
          disabled={saveState === "saving"}
          onClick={() => onDeleteNode(node.id)}
        >
          <Trash2 size={15} />
          <span>{saveState === "saving" ? "Deleting" : "Delete Node"}</span>
        </button>
      </div>
      {selectedDiagnostic && <SelectedDiagnostic diagnostic={selectedDiagnostic} />}
      <NodeRuntimeSection
        node={node}
        runtimeSnapshot={runtimeSnapshot}
        runtimeCounters={runtimeCounters}
        runtimeMessageActivity={runtimeMessageActivity}
      />
      <InspectorSection title="Overview" defaultOpen>
        {processorDescriptor && (
          <DescriptorSummary descriptor={processorDescriptor} configuredCount={node.parameters.length} />
        )}
        <KeyValue label="Kind" value={node.kind} />
        <KeyValue label="Config path" value={node.config_path} />
        <KeyValue label="File" value={configPath} />
        {node.pipeline_name && <KeyValue label="Pipeline" value={node.pipeline_name} />}
        <KeyValue label="Node ID" value={node.id} />
        <KeyValue label="Incoming" value={String(incomingEdges.length)} />
        <KeyValue label="Outgoing" value={String(outgoingEdges.length)} />
      </InspectorSection>

      <InspectorSection title="Channels" defaultOpen>
        <div className="inspector-subsection">
          <h4>Inputs</h4>
          {node.input_channels.length === 0 ? (
            <p className="empty-state">No input channels.</p>
          ) : (
            <ChannelButtonList channels={node.input_channels} onSelectChannel={onSelectChannel} />
          )}
        </div>
        <div className="inspector-subsection">
          <h4>Output</h4>
          {node.output_channel ? (
            <ChannelButtonList channels={[node.output_channel]} onSelectChannel={onSelectChannel} />
          ) : (
            <p className="empty-state">No output channel.</p>
          )}
        </div>
      </InspectorSection>

      <InspectorSection title="Stage Fields">
        <div className="parameter-list">
          {node.output_channel ? (
            <>
              <EditableFieldRow
                label="output"
                value={node.output_channel}
                valueKind="string"
                saveState={saveState}
                onSave={(value) => onUpdateField(node.id, "output", value)}
              />
              <EditableFieldRow
                label="channel.type"
                value={outputChannel?.channel_type ?? "broadcast"}
                valueKind="enum"
                options={["broadcast", "direct", "shared", "fanout"]}
                saveState={saveState}
                onSave={(value) => onUpdateField(node.id, "channel.type", value)}
              />
              <EditableFieldRow
                label="channel.capacity"
                value={String(outputChannel?.capacity ?? 128)}
                valueKind="number"
                saveState={saveState}
                onSave={(value) => onUpdateField(node.id, "channel.capacity", value)}
              />
            </>
          ) : (
            <p className="empty-state">No output or channel fields on this node.</p>
          )}
          <EditableFieldRow
            label="concurrency.type"
            value={node.concurrency_type}
            valueKind="enum"
            options={["thread", "pipeline", "owner"]}
            status={node.concurrency_type === "thread" ? "stable" : "reserved"}
            help="Only thread execution is currently implemented; pipeline and owner are reserved."
            statusNote="Pipeline and owner modes are preserved in config but currently run with thread semantics."
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "concurrency.type", value)}
          />
        </div>
      </InspectorSection>

      <InspectorSection title="Timing" badge={node.timing.length > 0 ? String(node.timing.length) : undefined}>
        <div className="parameter-list">
          <EditableFieldRow
            label="timing.event_time_field"
            value={timingValue(node, "event_time_field", "")}
            valueKind="string"
            status="experimental"
            statusNote="RFC3339 strings with Z or timezone offsets are supported; missing or unparsable values fall back to system time."
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "timing.event_time_field", value)}
          />
          <EditableFieldRow
            label="timing.max_lateness_ms"
            value={timingValue(node, "max_lateness_ms", "30000")}
            valueKind="number"
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "timing.max_lateness_ms", value)}
          />
          <EditableFieldRow
            label="timing.processing_timeout_ms"
            value={timingValue(node, "processing_timeout_ms", "")}
            valueKind="number"
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "timing.processing_timeout_ms", value)}
          />
          <EditableFieldRow
            label="timing.jitter_bounds_ms"
            value={timingValue(node, "jitter_bounds_ms", "")}
            valueKind="number"
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "timing.jitter_bounds_ms", value)}
          />
          <EditableFieldRow
            label="timing.metrics_enabled"
            value={timingValue(node, "metrics_enabled", "true")}
            valueKind="boolean"
            status="reserved"
            statusNote="Metrics flags are preserved for Phase 10 runtime events; no metrics collector consumes this yet."
            saveState={saveState}
            onSave={(value) => onUpdateField(node.id, "timing.metrics_enabled", value)}
          />
          {node.timing.some((field) => field.key === "watermark_strategy") && (
            <div className="parameter-row read-only">
              <div className="parameter-label">
                <strong>timing.watermark_strategy</strong>
                <span>object</span>
              </div>
              <FieldStatusBadge
                status="experimental"
                note="Watermark config is parsed, but global runtime watermark coordination is deferred."
              />
              <pre>configured</pre>
            </div>
          )}
        </div>
      </InspectorSection>

      <InspectorSection
        title="Parameters"
        badge={String(Math.max(node.parameters.length, knownParameterCount))}
      >
        {node.parameters.length === 0 && missingParameterFields.length === 0 ? (
          <p className="empty-state">No processor parameters.</p>
        ) : (
          <div className="parameter-list">
            {node.parameters.map((parameter) => {
              const fieldSpec = parameterFields.find((field) => field.key === parameter.key);
              return (
                <ParameterRow
                  key={parameter.key}
                  nodeId={node.id}
                  parameter={parameter}
                  fieldSpec={fieldSpec}
                  applies={!fieldSpec || fieldApplies(fieldSpec, nodeValues)}
                  inapplicableReason={fieldSpec && inapplicableReason(fieldSpec, nodeValues)}
                  saveState={saveState}
                  onUpdateParameter={onUpdateParameter}
                  onUpdateParameterJson={onUpdateParameterJson}
                  onRemoveParameter={onRemoveParameter}
                />
              );
            })}
            {missingParameterFields.map((field) => (
              <MissingParameterRow
                key={field.key}
                nodeId={node.id}
                field={field}
                saveState={saveState}
                onUpdateParameterJson={onUpdateParameterJson}
              />
            ))}
          </div>
        )}
      </InspectorSection>

      <DiagnosticsList diagnostics={diagnostics} selectedDiagnostic={selectedDiagnostic} defaultOpen={Boolean(selectedDiagnostic)} />
    </div>
  );
}

function NodeRuntimeSection({
  node,
  runtimeSnapshot,
  runtimeCounters,
  runtimeMessageActivity,
}: {
  node: GraphNode;
  runtimeSnapshot: RuntimeStageSnapshot | null;
  runtimeCounters: RuntimeStageCounters[string] | null;
  runtimeMessageActivity: RuntimeMessageActivity;
}) {
  const stageName = stageNameForNode(node);
  const stageActivityMs = stageName ? runtimeMessageActivity.stageIds[stageName] : undefined;
  const channelActivities = [...node.input_channels, node.output_channel ?? ""]
    .filter(Boolean)
    .map((channelName) => ({
      channelName,
      timestampMs: runtimeMessageActivity.channelNames[channelName],
    }))
    .filter((activity) => activity.timestampMs !== undefined)
    .sort((left, right) => right.timestampMs - left.timestampMs);
  const latestActivityMs = Math.max(
    stageActivityMs ?? 0,
    ...channelActivities.map((activity) => activity.timestampMs),
  );

  return (
    <InspectorSection title="Runtime" badge={runtimeSnapshot?.state} defaultOpen>
      {runtimeSnapshot ? (
        <div className="runtime-inspector">
          <div className="runtime-inspector-status">
            <span className={`runtime-inspector-badge ${runtimeSnapshot.state}`}>
              {runtimeSnapshot.state}
            </span>
            <span>{runtimeSnapshot.processorType ?? node.processor_type}</span>
          </div>
          {runtimeSnapshot.message && <p className="runtime-inspector-message">{runtimeSnapshot.message}</p>}
          <KeyValue label="Stage" value={stageName ?? node.display_name} />
          <KeyValue label="Received" value={formatRuntimeCount(runtimeCounters?.received ?? 0)} />
          <KeyValue label="Emitted" value={formatRuntimeCount(runtimeCounters?.emitted ?? 0)} />
          {(runtimeCounters?.errors ?? 0) > 0 && (
            <KeyValue label="Errors" value={formatRuntimeCount(runtimeCounters?.errors ?? 0)} />
          )}
          <KeyValue label="State updated" value={formatRuntimeTime(runtimeSnapshot.updatedAtMs)} />
          <KeyValue
            label="Last activity"
            value={latestActivityMs > 0 ? formatRuntimeTime(latestActivityMs) : "No messages observed."}
          />
          {channelActivities.length > 0 && (
            <div className="runtime-channel-activity">
              {channelActivities.slice(0, 4).map((activity) => (
                <div key={activity.channelName}>
                  <span>{activity.channelName}</span>
                  <strong>{formatRuntimeTime(activity.timestampMs)}</strong>
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        <p className="empty-state">No runtime events for this node yet.</p>
      )}
    </InspectorSection>
  );
}

function ChannelRuntimeSection({
  channel,
  runtimeMessageActivity,
  runtimeChannelCounters,
}: {
  channel: GraphChannel;
  runtimeMessageActivity: RuntimeMessageActivity;
  runtimeChannelCounters: RuntimeChannelCounters;
}) {
  const activityMs = runtimeMessageActivity.channelNames[channel.name];
  const counters = runtimeChannelCounters[channel.name];

  return (
    <InspectorSection
      title="Runtime"
      badge={activityMs || counters ? "active" : undefined}
      defaultOpen
    >
      {activityMs || counters ? (
        <div className="runtime-inspector">
          <div className="runtime-inspector-status">
            <span
              className={activityMs ? "runtime-inspector-badge running" : "runtime-inspector-badge"}
            >
              {activityMs ? "active" : "seen"}
            </span>
            <span>{channel.channel_type}</span>
          </div>
          <KeyValue label="Received" value={formatRuntimeCount(counters?.received ?? 0)} />
          <KeyValue label="Emitted" value={formatRuntimeCount(counters?.emitted ?? 0)} />
          <KeyValue
            label="Last activity"
            value={activityMs ? formatRuntimeTime(activityMs) : "No recent activity."}
          />
          <KeyValue label="Producers" value={String(channel.producer_node_ids.length)} />
          <KeyValue label="Consumers" value={String(channel.consumer_node_ids.length)} />
        </div>
      ) : (
        <p className="empty-state">No runtime messages observed on this channel yet.</p>
      )}
    </InspectorSection>
  );
}

function EdgeInspector({
  edge,
  graph,
  saveState,
  onSelectNode,
  onSelectChannel,
  onDisconnectEdge,
}: {
  edge: GraphEdge;
  graph: ResolvedPipelineGraph;
  saveState: SaveState;
  onSelectNode: (id: string | null) => void;
  onSelectChannel: (channelName: string | null) => void;
  onDisconnectEdge: (edge: GraphEdge) => Promise<void>;
}) {
  const sourceNode = graph.nodes.find((node) => node.id === edge.source_node_id) ?? null;
  const targetNode = graph.nodes.find((node) => node.id === edge.target_node_id) ?? null;
  const diagnostics = graph.diagnostics.filter((diagnostic) => diagnostic.channel_name === edge.channel_name);

  return (
    <div className="inspector-body">
      <InspectorTitle eyebrow="Connection" title={edge.channel_name} />
      <div className="edge-actions">
        <button
          className="danger-action"
          disabled={saveState === "saving"}
          onClick={() => onDisconnectEdge(edge)}
        >
          <Trash2 size={15} />
          <span>{saveState === "saving" ? "Disconnecting" : "Disconnect"}</span>
        </button>
      </div>
      <InspectorSection title="Endpoints" defaultOpen>
        <div className="endpoint-list">
          <button onClick={() => onSelectNode(edge.source_node_id)}>
            <span>Source</span>
            <strong>{sourceNode?.display_name ?? edge.source_node_id}</strong>
          </button>
          <button onClick={() => onSelectNode(edge.target_node_id)}>
            <span>Target</span>
            <strong>{targetNode?.display_name ?? edge.target_node_id}</strong>
          </button>
          <button onClick={() => onSelectChannel(edge.channel_name)}>
            <span>Channel</span>
            <strong>{edge.channel_name}</strong>
          </button>
        </div>
        <KeyValue label="Target input" value={String(edge.target_input_index + 1)} />
      </InspectorSection>
      <DiagnosticsList diagnostics={diagnostics} selectedDiagnostic={null} defaultOpen={diagnostics.length > 0} />
    </div>
  );
}

function ChannelInspector({
  channel,
  graph,
  diagnostics,
  selectedDiagnostic,
  runtimeMessageActivity,
  runtimeChannelCounters,
  onSelectNode,
}: {
  channel: GraphChannel;
  graph: ResolvedPipelineGraph;
  diagnostics: GraphDiagnostic[];
  selectedDiagnostic: GraphDiagnostic | null;
  runtimeMessageActivity: RuntimeMessageActivity;
  runtimeChannelCounters: RuntimeChannelCounters;
  onSelectNode: (id: string | null) => void;
}) {
  return (
    <div className="inspector-body">
      <InspectorTitle eyebrow="Channel" title={channel.name} />
      {selectedDiagnostic && <SelectedDiagnostic diagnostic={selectedDiagnostic} />}
      <ChannelRuntimeSection
        channel={channel}
        runtimeMessageActivity={runtimeMessageActivity}
        runtimeChannelCounters={runtimeChannelCounters}
      />
      <KeyValue label="Type" value={channel.channel_type} />
      <KeyValue label="Capacity" value={String(channel.capacity)} />

      <InspectorSection title="Producers" badge={String(channel.producer_node_ids.length)} defaultOpen>
        <NodeButtonList nodeIds={channel.producer_node_ids} graph={graph} onSelectNode={onSelectNode} />
      </InspectorSection>

      <InspectorSection title="Consumers" badge={String(channel.consumer_node_ids.length)} defaultOpen>
        <NodeButtonList nodeIds={channel.consumer_node_ids} graph={graph} onSelectNode={onSelectNode} />
      </InspectorSection>

      <DiagnosticsList diagnostics={diagnostics} selectedDiagnostic={selectedDiagnostic} defaultOpen={Boolean(selectedDiagnostic)} />
    </div>
  );
}

function MissingChannelInspector({
  channelName,
  diagnostics,
  selectedDiagnostic,
}: {
  channelName: string;
  diagnostics: GraphDiagnostic[];
  selectedDiagnostic: GraphDiagnostic | null;
}) {
  return (
    <div className="inspector-body">
      <InspectorTitle eyebrow="Unresolved Channel" title={channelName} />
      {selectedDiagnostic && <SelectedDiagnostic diagnostic={selectedDiagnostic} />}
      <DiagnosticsList diagnostics={diagnostics} selectedDiagnostic={selectedDiagnostic} defaultOpen />
    </div>
  );
}

function timingValue(node: GraphNode, key: string, fallback: string) {
  return node.timing.find((field) => field.key === key)?.value ?? fallback;
}

function runtimeStageSnapshotForNode(
  node: GraphNode,
  runtimeStageStates: RuntimeStageStates,
) {
  const stageName = stageNameForNode(node);
  return stageName ? runtimeStageStates[stageName] ?? null : null;
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

function formatRuntimeTime(timestampMs: number) {
  if (!Number.isFinite(timestampMs) || timestampMs <= 0) {
    return "Unknown";
  }

  const date = new Date(timestampMs);
  return `${date.toLocaleTimeString()} (${relativeRuntimeAge(timestampMs)})`;
}

function formatRuntimeCount(count: number) {
  return new Intl.NumberFormat().format(count);
}

function relativeRuntimeAge(timestampMs: number) {
  const elapsedMs = Math.max(0, Date.now() - timestampMs);
  if (elapsedMs < 1000) {
    return "just now";
  }
  if (elapsedMs < 60_000) {
    return `${Math.round(elapsedMs / 1000)}s ago`;
  }
  return `${Math.round(elapsedMs / 60_000)}m ago`;
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
