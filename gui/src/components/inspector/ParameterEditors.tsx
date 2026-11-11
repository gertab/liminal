import { ArrowDown, ArrowUp, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { FieldStatusBadge } from "./InspectorPrimitives";
import { fieldApplies, objectValues } from "./fieldConditions";
import {
  FieldKind,
  FieldSpec,
  FieldStatus,
  GraphParameter,
  JsonValue,
  ProcessorDescriptor,
  SaveState,
  SchemaSpec,
} from "../../types";

type ValidationIssue = {
  path: string;
  message: string;
};
const conditionOperationOptions = [
  "always",
  "never",
  "equals",
  "not_equals",
  "startswith",
  "endswith",
  "contains",
  ">",
  ">=",
  "<",
  "<=",
];
export function ParameterRow({
  nodeId,
  parameter,
  fieldSpec,
  applies = true,
  inapplicableReason,
  saveState,
  onUpdateParameter,
  onUpdateParameterJson,
  onRemoveParameter,
}: {
  nodeId: string;
  parameter: GraphParameter;
  fieldSpec?: FieldSpec;
  /** Whether the field applies, given the node's other parameters (`FieldSpec.applies_when`). */
  applies?: boolean;
  inapplicableReason?: string;
  saveState: SaveState;
  onUpdateParameter: (nodeId: string, parameterKey: string, value: string) => Promise<void>;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
  onRemoveParameter?: (nodeId: string, parameterKey: string) => Promise<void>;
}) {
  const [draftValue, setDraftValue] = useState(parameter.value);
  const isDirty = draftValue !== parameter.value;
  const label = fieldSpec?.label ?? parameter.key;
  const fieldKind = editableKindForParameter(parameter, fieldSpec);
  const valueKind = fieldSpec?.kind ?? parameter.value_kind;
  const options = fieldSpec ? selectOptionsForValue(fieldSpec, draftValue) : [];
  const status = fieldSpec?.status ?? "stable";
  const statusNote = fieldSpec?.status_note ?? null;
  // A parameter can go if it's optional, doesn't apply, or isn't one the processor knows.
  const canRemove = onRemoveParameter !== undefined && (!applies || !fieldSpec?.required);

  useEffect(() => {
    setDraftValue(parameter.value);
  }, [parameter.value]);

  const header = (
    <>
      <div className="parameter-label">
        <strong title={parameter.key}>{label}</strong>
        <span>{valueKind}</span>
        {canRemove && (
          <button
            className="icon-button danger parameter-remove"
            disabled={saveState === "saving"}
            onClick={() => onRemoveParameter(nodeId, parameter.key)}
            aria-label={`Remove ${parameter.key}`}
            title={fieldSpec ? "Remove (the default applies)" : "Remove (not a known parameter)"}
          >
            <Trash2 size={13} />
          </button>
        )}
      </div>
      <FieldStatusBadge status={status} note={statusNote} />
      {!applies && <p className="parameter-note">Not used: {inapplicableReason}.</p>}
      {!fieldSpec && <p className="parameter-note">Not a parameter of this processor.</p>}
      {fieldSpec?.help && <p className="parameter-help">{fieldSpec.help}</p>}
    </>
  );

  if (!parameter.editable) {
    return (
      <div
        className={
          fieldSpec?.renderer === "rule_builder" || fieldSpec?.renderer === "object_list"
            ? "parameter-row read-only rule-parameter-row"
            : "parameter-row read-only"
        }
      >
        {header}
        {fieldSpec?.renderer === "object_list" && fieldSpec.schema ? (
          <ObjectListParameterEditor
            nodeId={nodeId}
            parameterKey={parameter.key}
            value={parameter.raw_value}
            schema={fieldSpec.schema}
            saveState={saveState}
            onUpdateParameterJson={onUpdateParameterJson}
          />
        ) : fieldSpec?.renderer === "rule_builder" && fieldSpec.schema ? (
          <RuleParameterEditor
            nodeId={nodeId}
            parameterKey={parameter.key}
            value={parameter.raw_value}
            schema={fieldSpec.schema}
            saveState={saveState}
            onUpdateParameterJson={onUpdateParameterJson}
          />
        ) : fieldSpec?.renderer === "string_array" ? (
          <StringArrayParameterEditor
            nodeId={nodeId}
            parameterKey={parameter.key}
            value={parameter.raw_value}
            saveState={saveState}
            onUpdateParameterJson={onUpdateParameterJson}
          />
        ) : fieldSpec?.schema ? (
          <NestedParameterPreview value={parameter.raw_value} schema={fieldSpec.schema} />
        ) : (
          <pre>{parameter.value}</pre>
        )}
      </div>
    );
  }

  return (
    <div className={isDirty ? "parameter-row dirty" : "parameter-row"}>
      {header}
      {fieldKind === "enum" ? (
        <select value={draftValue} onChange={(event) => setDraftValue(event.target.value)}>
          {options.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      ) : fieldKind === "boolean" ? (
        <label className="parameter-toggle">
          <input
            type="checkbox"
            checked={draftValue === "true"}
            onChange={(event) => setDraftValue(String(event.target.checked))}
          />
          <span>{draftValue}</span>
        </label>
      ) : (
        <input
          value={draftValue}
          type={fieldKind === "number" ? "number" : "text"}
          onChange={(event) => setDraftValue(event.target.value)}
        />
      )}
      <button
        className="parameter-save"
        disabled={!isDirty || saveState === "saving"}
        onClick={() => onUpdateParameter(nodeId, parameter.key, draftValue)}
      >
        {saveState === "saving" ? "Saving" : "Save"}
      </button>
    </div>
  );
}

export function MissingParameterRow({
  nodeId,
  field,
  saveState,
  onUpdateParameterJson,
}: {
  nodeId: string;
  field: FieldSpec;
  saveState: SaveState;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
}) {
  if (field.renderer === "rule_builder" && field.schema) {
    return (
      <div className="parameter-row read-only missing-parameter rule-parameter-row">
        <div className="parameter-label">
          <strong title={field.key}>{field.label}</strong>
          <span>{field.kind}</span>
        </div>
        <FieldStatusBadge status={field.status} note={field.status_note} />
        {field.help && <p className="parameter-help">{field.help}</p>}
        <RuleParameterEditor
          nodeId={nodeId}
          parameterKey={field.key}
          value={[]}
          schema={field.schema}
          saveState={saveState}
          onUpdateParameterJson={onUpdateParameterJson}
        />
      </div>
    );
  }

  if (field.renderer === "string_array") {
    return (
      <div className="parameter-row read-only missing-parameter rule-parameter-row">
        <div className="parameter-label">
          <strong title={field.key}>{field.label}</strong>
          <span>{field.kind}</span>
        </div>
        <FieldStatusBadge status={field.status} note={field.status_note} />
        {field.help && <p className="parameter-help">{field.help}</p>}
        <StringArrayParameterEditor
          nodeId={nodeId}
          parameterKey={field.key}
          value={defaultValueForField(field)}
          saveState={saveState}
          onUpdateParameterJson={onUpdateParameterJson}
        />
      </div>
    );
  }

  if (field.renderer === "object_list" && field.schema) {
    return (
      <div className="parameter-row read-only missing-parameter rule-parameter-row">
        <div className="parameter-label">
          <strong title={field.key}>{field.label}</strong>
          <span>{field.kind}</span>
        </div>
        <FieldStatusBadge status={field.status} note={field.status_note} />
        {field.help && <p className="parameter-help">{field.help}</p>}
        <ObjectListParameterEditor
          nodeId={nodeId}
          parameterKey={field.key}
          value={[]}
          schema={field.schema}
          saveState={saveState}
          onUpdateParameterJson={onUpdateParameterJson}
        />
      </div>
    );
  }

  const defaultValue = defaultValueForField(field);

  return (
    <div className="parameter-row read-only missing-parameter">
      <div className="parameter-label">
        <strong title={field.key}>{field.label}</strong>
        <span>{field.kind}</span>
      </div>
      <FieldStatusBadge status={field.status} note={field.status_note} />
      {field.help && <p className="parameter-help">{field.help}</p>}
      <div className="parameter-default">
        <span>{field.required ? "required" : "default"}</span>
        <strong>{field.default_value ?? "not set"}</strong>
      </div>
      <button
        className="parameter-placeholder-action"
        disabled={saveState === "saving"}
        onClick={() => onUpdateParameterJson(nodeId, field.key, defaultValue)}
      >
        {field.required ? "Configure" : "Set default"}
      </button>
    </div>
  );
}

function StringArrayParameterEditor({
  nodeId,
  parameterKey,
  value,
  saveState,
  onUpdateParameterJson,
}: {
  nodeId: string;
  parameterKey: string;
  value: JsonValue;
  saveState: SaveState;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
}) {
  const arrayValue = useMemo(
    () => (Array.isArray(value) ? value.map((item) => formatJsonValue(item)) : []),
    [value],
  );
  const [draftItems, setDraftItems] = useState<string[]>(arrayValue);
  const isDirty = JSON.stringify(draftItems) !== JSON.stringify(arrayValue);

  useEffect(() => {
    setDraftItems(arrayValue);
  }, [arrayValue]);

  return (
    <div className="string-array-editor">
      <div className="string-array-list">
        {draftItems.length === 0 ? (
          <p className="empty-state">No values configured.</p>
        ) : (
          draftItems.map((item, index) => (
            <div className="string-array-row" key={index}>
              <input
                value={item}
                aria-label={`${parameterKey} item ${index + 1}`}
                onChange={(event) =>
                  setDraftItems((currentItems) =>
                    currentItems.map((currentItem, currentIndex) =>
                      currentIndex === index ? event.target.value : currentItem,
                    ),
                  )
                }
              />
              <button
                className="icon-button danger"
                onClick={() =>
                  setDraftItems((currentItems) =>
                    currentItems.filter((_, currentIndex) => currentIndex !== index),
                  )
                }
                aria-label={`Remove ${parameterKey} item ${index + 1}`}
                title="Remove"
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))
        )}
      </div>
      <div className="string-array-actions">
        <button
          className="compact-add-button"
          onClick={() => setDraftItems((currentItems) => [...currentItems, ""])}
        >
          Add
        </button>
        <button disabled={!isDirty || saveState === "saving"} onClick={() => setDraftItems(arrayValue)}>
          Revert
        </button>
        <button
          disabled={!isDirty || saveState === "saving"}
          onClick={() => onUpdateParameterJson(nodeId, parameterKey, draftItems)}
        >
          {saveState === "saving" ? "Saving" : "Save"}
        </button>
      </div>
    </div>
  );
}

type JsonObject = { [key: string]: JsonValue };

/**
 * Edits an array of flat objects (`renderer = "object_list"`), described by the field's schema: an
 * `array` of `object` with scalar fields. One row per item, one input per field that applies to the
 * item (`applies_when` on the item's other fields, e.g. no field for a count).
 */
function ObjectListParameterEditor({
  nodeId,
  parameterKey,
  value,
  schema,
  saveState,
  onUpdateParameterJson,
}: {
  nodeId: string;
  parameterKey: string;
  value: JsonValue;
  schema: SchemaSpec;
  saveState: SaveState;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
}) {
  const itemFields = useMemo(
    () => (schema.kind === "array" && schema.item.kind === "object" ? schema.item.fields : []),
    [schema],
  );
  const savedItems = useMemo(() => (Array.isArray(value) ? value.filter(isJsonObject) : []), [value]);
  const [draftItems, setDraftItems] = useState<JsonObject[]>(savedItems);
  const isDirty = JSON.stringify(draftItems) !== JSON.stringify(savedItems);
  const issues = useMemo(() => validateObjectList(draftItems, itemFields), [draftItems, itemFields]);

  useEffect(() => {
    setDraftItems(savedItems);
  }, [savedItems]);

  const updateItem = (index: number, nextItem: JsonObject) =>
    setDraftItems((items) =>
      items.map((item, itemIndex) => (itemIndex === index ? withoutInapplicable(nextItem, itemFields) : item)),
    );

  return (
    <div className="object-list-editor">
      {draftItems.length === 0 ? (
        <p className="empty-state">No items configured.</p>
      ) : (
        draftItems.map((item, index) => (
          <div className="object-list-row" key={index}>
            <div className="object-list-fields">
              {applicableFields(item, itemFields).map((field) => (
                <label className="object-list-field" key={field.key} title={field.help}>
                  <span>
                    {field.label}
                    {field.required && " *"}
                  </span>
                  <ObjectFieldInput
                    field={field}
                    value={item[field.key]}
                    ariaLabel={`${parameterKey} item ${index + 1} ${field.key}`}
                    onChange={(nextValue) => updateItem(index, withObjectField(item, field.key, nextValue))}
                  />
                </label>
              ))}
            </div>
            <div className="object-list-row-actions">
              <button
                className="icon-button"
                disabled={index === 0}
                onClick={() => setDraftItems((items) => moveArrayItem(items, index, -1))}
                aria-label={`Move ${parameterKey} item ${index + 1} up`}
                title="Move up"
              >
                <ArrowUp size={13} />
              </button>
              <button
                className="icon-button"
                disabled={index === draftItems.length - 1}
                onClick={() => setDraftItems((items) => moveArrayItem(items, index, 1))}
                aria-label={`Move ${parameterKey} item ${index + 1} down`}
                title="Move down"
              >
                <ArrowDown size={13} />
              </button>
              <button
                className="icon-button danger"
                onClick={() => setDraftItems((items) => items.filter((_, itemIndex) => itemIndex !== index))}
                aria-label={`Remove ${parameterKey} item ${index + 1}`}
                title="Remove"
              >
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        ))
      )}
      {issues.length > 0 && (
        <ul className="object-list-issues">
          {issues.map((issue) => (
            <li key={issue}>{issue}</li>
          ))}
        </ul>
      )}
      <div className="string-array-actions">
        <button
          className="compact-add-button"
          onClick={() => setDraftItems((items) => [...items, defaultObjectItem(itemFields)])}
        >
          Add
        </button>
        <button disabled={!isDirty || saveState === "saving"} onClick={() => setDraftItems(savedItems)}>
          Revert
        </button>
        <button
          disabled={!isDirty || saveState === "saving" || issues.length > 0}
          onClick={() => onUpdateParameterJson(nodeId, parameterKey, draftItems)}
        >
          {saveState === "saving" ? "Saving" : "Save"}
        </button>
      </div>
    </div>
  );
}

/** One scalar field of an object-list item. Clearing an optional field leaves it out of the item. */
function ObjectFieldInput({
  field,
  value,
  ariaLabel,
  onChange,
}: {
  field: FieldSpec;
  value: JsonValue | undefined;
  ariaLabel: string;
  onChange: (value: JsonValue | undefined) => void;
}) {
  if (field.kind === "enum") {
    const selected = typeof value === "string" ? value : "";
    return (
      <select
        aria-label={ariaLabel}
        value={selected}
        onChange={(event) => onChange(event.target.value === "" ? undefined : event.target.value)}
      >
        {(!field.required || selected === "") && (
          <option value="">{field.required ? "Choose…" : "(none)"}</option>
        )}
        {field.options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  }

  if (field.kind === "boolean") {
    return (
      <input
        type="checkbox"
        aria-label={ariaLabel}
        checked={value === true}
        onChange={(event) => onChange(event.target.checked)}
      />
    );
  }

  const isNumber = field.kind === "integer" || field.kind === "number";
  return (
    <input
      aria-label={ariaLabel}
      type={isNumber ? "number" : "text"}
      value={value === undefined || value === null ? "" : String(value)}
      placeholder={field.default_value ?? ""}
      onChange={(event) => {
        const text = event.target.value;
        if (text.trim() === "") {
          onChange(undefined);
        } else {
          onChange(isNumber ? Number(text) : text);
        }
      }}
    />
  );
}

/** The item with `key` set, keeping its place, or removed when `value` is undefined. */
function withObjectField(item: JsonObject, key: string, value: JsonValue | undefined): JsonObject {
  if (value !== undefined) {
    return { ...item, [key]: value };
  }
  const next = { ...item };
  delete next[key];
  return next;
}

/** The fields that apply to an item, given its other fields (`applies_when`). */
function applicableFields(item: JsonObject, fields: FieldSpec[]): FieldSpec[] {
  const values = objectValues(item, fields);
  return fields.filter((field) => fieldApplies(field, values));
}

/** The item without the fields that don't apply to it (e.g. a field left over after switching to count). */
function withoutInapplicable(item: JsonObject, fields: FieldSpec[]): JsonObject {
  const applicable = applicableFields(item, fields);
  return fields
    .filter((field) => !applicable.includes(field))
    .reduce((current, field) => withObjectField(current, field.key, undefined), item);
}

/** A new item: each required field that applies, at its default. */
function defaultObjectItem(fields: FieldSpec[]): JsonObject {
  const item: JsonObject = {};
  for (const field of fields) {
    if (field.required) {
      item[field.key] = defaultValueForField(field);
    }
  }
  return withoutInapplicable(item, fields);
}

function validateObjectList(items: JsonObject[], fields: FieldSpec[]): string[] {
  const issues: string[] = [];
  items.forEach((item, index) => {
    for (const field of applicableFields(item, fields)) {
      const value = item[field.key];
      if (field.required && isEmptyRequiredValue(value)) {
        issues.push(`Item ${index + 1}: ${field.label} is required.`);
      }
      if (field.kind === "integer" && typeof value === "number" && !Number.isInteger(value)) {
        issues.push(`Item ${index + 1}: ${field.label} must be a whole number.`);
      }
    }
  });
  return issues;
}

function RuleParameterEditor({
  nodeId,
  parameterKey,
  value,
  schema,
  saveState,
  onUpdateParameterJson,
}: {
  nodeId: string;
  parameterKey: string;
  value: JsonValue;
  schema: SchemaSpec;
  saveState: SaveState;
  onUpdateParameterJson: (nodeId: string, parameterKey: string, value: JsonValue) => Promise<void>;
}) {
  const [draftRules, setDraftRules] = useState<JsonValue[]>(Array.isArray(value) ? value : []);
  const actionSchema = ruleActionSchema(schema);
  const isDirty = JSON.stringify(draftRules) !== JSON.stringify(Array.isArray(value) ? value : []);
  const validationIssues = useMemo(
    () => validateRules(draftRules, actionSchema),
    [actionSchema, draftRules],
  );

  useEffect(() => {
    setDraftRules(Array.isArray(value) ? value : []);
  }, [value]);

  return (
    <div className="rule-editor">
      <div className="rule-editor-toolbar">
        <button
          onClick={() =>
            setDraftRules((currentRules) => [
              ...currentRules,
              defaultRule(actionSchema),
            ])
          }
        >
          Add Rule
        </button>
        <button disabled={!isDirty || saveState === "saving"} onClick={() => setDraftRules(Array.isArray(value) ? value : [])}>
          Revert
        </button>
        <button
          disabled={!isDirty || saveState === "saving" || validationIssues.length > 0}
          onClick={() => onUpdateParameterJson(nodeId, parameterKey, draftRules)}
        >
          {saveState === "saving" ? "Saving" : "Save Rules"}
        </button>
      </div>
      {validationIssues.length > 0 && <RuleValidationSummary issues={validationIssues} />}
      {draftRules.length === 0 ? (
        <p className="empty-state">No rules configured.</p>
      ) : (
        draftRules.map((rule, index) => (
          <RuleCard
            key={index}
            rule={rule}
            index={index}
            actionSchema={actionSchema}
            issues={validationIssues.filter((issue) => issue.path.startsWith(`rules.${index}.`))}
            onChange={(nextRule) => {
              setDraftRules((currentRules) =>
                currentRules.map((currentRule, currentIndex) => (currentIndex === index ? nextRule : currentRule)),
              );
            }}
            onMove={(direction) => setDraftRules((currentRules) => moveArrayItem(currentRules, index, direction))}
            onRemove={() =>
              setDraftRules((currentRules) => currentRules.filter((_, currentIndex) => currentIndex !== index))
            }
            canMoveUp={index > 0}
            canMoveDown={index < draftRules.length - 1}
          />
        ))
      )}
    </div>
  );
}

function RuleCard({
  rule,
  index,
  actionSchema,
  issues,
  onChange,
  onMove,
  onRemove,
  canMoveUp,
  canMoveDown,
}: {
  rule: JsonValue;
  index: number;
  actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null;
  issues: ValidationIssue[];
  onChange: (rule: JsonValue) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
  canMoveUp: boolean;
  canMoveDown: boolean;
}) {
  const ruleObject = isJsonObject(rule) ? rule : {};
  const condition = isJsonObject(ruleObject.condition) ? ruleObject.condition : {};
  const actions = Array.isArray(ruleObject.actions) ? ruleObject.actions : [];
  const elseActions = Array.isArray(ruleObject.else_actions) ? ruleObject.else_actions : [];
  const operation = typeof condition.operation === "string" ? condition.operation : "equals";
  const isUnconditional = operation === "always" || operation === "never";

  return (
    <div className="rule-card">
      <div className="rule-card-header">
        <span>Rule {index + 1}</span>
        <strong>{ruleSummary(condition, actions.length, elseActions.length)}</strong>
        <div className="rule-button-group">
          <button
            className="icon-button"
            disabled={!canMoveUp}
            onClick={() => onMove(-1)}
            aria-label={`Move rule ${index + 1} up`}
            title="Move up"
          >
            <ArrowUp size={13} />
          </button>
          <button
            className="icon-button"
            disabled={!canMoveDown}
            onClick={() => onMove(1)}
            aria-label={`Move rule ${index + 1} down`}
            title="Move down"
          >
            <ArrowDown size={13} />
          </button>
          <button className="icon-button danger" onClick={onRemove} aria-label={`Remove rule ${index + 1}`} title="Remove">
            <Trash2 size={13} />
          </button>
        </div>
      </div>

      <div className="rule-condition">
        <RuleSelect
          label="Operation"
          value={operation}
          options={conditionOperationOptions}
          issue={issueForPath(issues, `condition.operation`)}
          onChange={(nextValue) =>
            onChange(setObjectValue(ruleObject, ["condition", "operation"], nextValue))
          }
        />
        {!isUnconditional && (
          <>
            <RuleInput
              label="Field"
              value={formatJsonValue(condition.field_path ?? "")}
              issue={issueForPath(issues, `condition.field_path`)}
              onChange={(nextValue) =>
                onChange(setObjectValue(ruleObject, ["condition", "field_path"], nextValue))
              }
            />
            <RuleInput
              label="Value"
              value={formatJsonValue(condition.value ?? "")}
              issue={issueForPath(issues, `condition.value`)}
              onChange={(nextValue) =>
                onChange(setObjectValue(ruleObject, ["condition", "value"], parseJsonLikeValue(nextValue)))
              }
            />
          </>
        )}
      </div>

      <RuleActionList
        title="Actions"
        actions={actions}
        actionSchema={actionSchema}
        issues={issues.filter((issue) => issue.path.startsWith("actions."))}
        onChange={(nextActions) => onChange({ ...ruleObject, actions: nextActions })}
      />
      <RuleActionList
        title="Else Actions"
        actions={elseActions}
        actionSchema={actionSchema}
        issues={issues.filter((issue) => issue.path.startsWith("else_actions."))}
        onChange={(nextActions) => onChange({ ...ruleObject, else_actions: nextActions })}
      />
    </div>
  );
}

function RuleActionList({
  title,
  actions,
  actionSchema,
  issues,
  onChange,
}: {
  title: string;
  actions: JsonValue[];
  actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null;
  issues: ValidationIssue[];
  onChange: (actions: JsonValue[]) => void;
}) {
  return (
    <div className="rule-action-section">
      <div className="rule-action-section-header">
        <div className="rule-action-section-heading">
          <span>{title}</span>
          <strong>{actions.length}</strong>
        </div>
        <button
          className="compact-add-button"
          disabled={!actionSchema}
          onClick={() =>
            actionSchema && onChange([...actions, defaultActionForVariant(actionSchema, actionSchema.variants[0]?.tag_value ?? "")])
          }
        >
          Add
        </button>
      </div>
      {actions.length === 0 ? (
        <p className="empty-state">None.</p>
      ) : (
        <div className="rule-action-list">
          {actions.map((action, index) => (
            <RuleActionCard
              key={index}
              action={action}
              index={index}
              actionSchema={actionSchema}
              issues={issues
                .filter((issue) => issue.path.startsWith(`${index}.`))
                .map((issue) => ({ ...issue, path: issue.path.replace(`${index}.`, "") }))}
              onChange={(nextAction) => {
                onChange(
                  actions.map((currentAction, currentIndex) =>
                    currentIndex === index ? nextAction : currentAction,
                  ),
                );
              }}
              onMove={(direction) => onChange(moveArrayItem(actions, index, direction))}
              onRemove={() => onChange(actions.filter((_, currentIndex) => currentIndex !== index))}
              canMoveUp={index > 0}
              canMoveDown={index < actions.length - 1}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function RuleActionCard({
  action,
  index,
  actionSchema,
  issues,
  onChange,
  onMove,
  onRemove,
  canMoveUp,
  canMoveDown,
}: {
  action: JsonValue;
  index: number;
  actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null;
  issues: ValidationIssue[];
  onChange: (action: JsonValue) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
  canMoveUp: boolean;
  canMoveDown: boolean;
}) {
  const actionObject = isJsonObject(action) ? action : {};
  const type = typeof actionObject.type === "string" ? actionObject.type : "";
  const variant = actionSchema?.variants.find((candidate) => candidate.tag_value === type);
  const fields = variant?.fields ?? Object.keys(actionObject)
    .filter((key) => key !== "type")
    .map((key) => ({
      key,
      label: labelFromKey(key),
      kind: "json_value" as FieldKind,
      required: false,
      default_value: null,
      options: [],
      help: "",
      schema: null,
      renderer: null,
      status: "stable" as FieldStatus,
      status_note: null,
      applies_when: [],
    }));

  return (
    <div className="rule-action-card">
      <div className="rule-action-title">
        <span>Action {index + 1}</span>
        {actionSchema ? (
          <select
            className={issueForPath(issues, "type") ? "invalid" : ""}
            value={type}
            onChange={(event) => onChange(defaultActionForVariant(actionSchema, event.target.value))}
          >
            {actionSchema.variants.map((candidate) => (
              <option key={candidate.tag_value} value={candidate.tag_value}>
                {candidate.label}
              </option>
            ))}
          </select>
        ) : (
          <strong>{variant?.label ?? labelFromKey(type || "action")}</strong>
        )}
      </div>
      <div className="rule-action-fields">
        {fields.map((field) => (
          <RuleFieldEditor
            key={field.key}
            field={field}
            value={actionObject[field.key] ?? ""}
            issue={issueForPath(issues, field.key)}
            onChange={(nextValue) => onChange({ ...actionObject, [field.key]: nextValue })}
          />
        ))}
      </div>
      <div className="rule-button-group rule-row-controls">
        <button
          className="icon-button"
          disabled={!canMoveUp}
          onClick={() => onMove(-1)}
          aria-label={`Move action ${index + 1} up`}
          title="Move up"
        >
          <ArrowUp size={13} />
        </button>
        <button
          className="icon-button"
          disabled={!canMoveDown}
          onClick={() => onMove(1)}
          aria-label={`Move action ${index + 1} down`}
          title="Move down"
        >
          <ArrowDown size={13} />
        </button>
        <button className="icon-button danger" onClick={onRemove} aria-label={`Remove action ${index + 1}`} title="Remove">
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  );
}

function RuleValidationSummary({ issues }: { issues: ValidationIssue[] }) {
  return (
    <div className="rule-validation">
      <strong>{issues.length} issue{issues.length === 1 ? "" : "s"}</strong>
      {issues.slice(0, 4).map((issue) => (
        <p key={`${issue.path}-${issue.message}`}>{issue.message}</p>
      ))}
      {issues.length > 4 && <p>{issues.length - 4} more.</p>}
    </div>
  );
}

function RuleFieldEditor({
  field,
  value,
  issue,
  onChange,
}: {
  field: FieldSpec;
  value: JsonValue;
  issue?: ValidationIssue;
  onChange: (value: JsonValue) => void;
}) {
  if (field.kind === "boolean") {
    return (
      <label className={["rule-datum", "rule-checkbox", issue ? "invalid" : ""].join(" ")}>
        <span>{field.label}</span>
        <input
          type="checkbox"
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
        {issue && <small>{issue.message}</small>}
      </label>
    );
  }

  if (field.kind === "enum" && field.options.length > 0) {
    return (
      <label className={["rule-datum", issue ? "invalid" : ""].join(" ")}>
        <span>{field.label}</span>
        <select
          value={typeof value === "string" ? value : field.options[0]}
          onChange={(event) => onChange(event.target.value)}
        >
          {field.options.map((option) => (
            <option key={option} value={option}>
            {option}
          </option>
        ))}
        </select>
        {issue && <small>{issue.message}</small>}
      </label>
    );
  }

  return (
    <RuleInput
      label={field.label}
      value={formatJsonValue(value)}
      issue={issue}
      onChange={(nextValue) =>
        onChange(["array", "object", "json_value"].includes(field.kind) ? parseJsonLikeValue(nextValue) : nextValue)
      }
    />
  );
}

function RuleInput({
  label,
  value,
  issue,
  onChange,
}: {
  label: string;
  value: string;
  issue?: ValidationIssue;
  onChange: (value: string) => void;
}) {
  return (
    <label className={["rule-datum", issue ? "invalid" : ""].join(" ")}>
      <span>{label}</span>
      <input className={issue ? "invalid" : ""} value={value} onChange={(event) => onChange(event.target.value)} />
      {issue && <small>{issue.message}</small>}
    </label>
  );
}

function RuleSelect({
  label,
  value,
  options,
  issue,
  onChange,
}: {
  label: string;
  value: string;
  options: string[];
  issue?: ValidationIssue;
  onChange: (value: string) => void;
}) {
  return (
    <label className={["rule-datum", issue ? "invalid" : ""].join(" ")}>
      <span>{label}</span>
      <select className={issue ? "invalid" : ""} value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
      {issue && <small>{issue.message}</small>}
    </label>
  );
}

function NestedParameterPreview({
  value,
  schema,
}: {
  value: JsonValue;
  schema: SchemaSpec;
}) {
  if (schema.kind === "array") {
    const items = Array.isArray(value) ? value : [];

    return (
      <div className="nested-preview">
        {items.length === 0 ? (
          <p className="empty-state">Empty array.</p>
        ) : (
          items.map((item, index) => (
            <NestedItem key={index} title={`Item ${index + 1}`} value={item} schema={schema.item} />
          ))
        )}
      </div>
    );
  }

  if (schema.kind === "object") {
    return <NestedObject value={value} fields={schema.fields} />;
  }

  if (schema.kind === "tagged_union") {
    return <NestedTaggedUnion value={value} schema={schema} />;
  }

  return <pre>{formatJsonValue(value)}</pre>;
}

function NestedItem({
  title,
  value,
  schema,
}: {
  title: string;
  value: JsonValue;
  schema: SchemaSpec;
}) {
  return (
    <div className="nested-item">
      <div className="nested-item-title">
        <span>{title}</span>
        <strong>{summarizeNestedValue(value, schema)}</strong>
      </div>
      <NestedParameterPreview value={value} schema={schema} />
    </div>
  );
}

function NestedObject({ value, fields }: { value: JsonValue; fields: FieldSpec[] }) {
  const objectValue = isJsonObject(value) ? value : {};

  return (
    <div className="nested-object">
      {fields.map((field) => {
        const childValue = objectValue[field.key] ?? null;

        return (
          <div className="nested-field" key={field.key}>
            <span>{field.label}</span>
            {field.schema ? (
              <NestedParameterPreview value={childValue} schema={field.schema} />
            ) : (
              <strong>{formatJsonValue(childValue)}</strong>
            )}
          </div>
        );
      })}
    </div>
  );
}

function NestedTaggedUnion({
  value,
  schema,
}: {
  value: JsonValue;
  schema: Extract<SchemaSpec, { kind: "tagged_union" }>;
}) {
  const objectValue = isJsonObject(value) ? value : {};
  const tagValue = objectValue[schema.tag];
  const variant = schema.variants.find((candidate) => candidate.tag_value === tagValue);

  return (
    <div className="nested-object tagged-union">
      <div className="nested-field">
        <span>{schema.tag}</span>
        <strong>{variant?.label ?? formatJsonValue(tagValue)}</strong>
      </div>
      {(variant?.fields ?? []).map((field) => (
        <div className="nested-field" key={field.key}>
          <span>{field.label}</span>
          {field.schema ? (
            <NestedParameterPreview value={objectValue[field.key] ?? null} schema={field.schema} />
          ) : (
            <strong>{formatJsonValue(objectValue[field.key] ?? null)}</strong>
          )}
        </div>
      ))}
    </div>
  );
}

export function DescriptorSummary({
  descriptor,
  configuredCount,
}: {
  descriptor: ProcessorDescriptor;
  configuredCount: number;
}) {
  return (
    <div className="descriptor-summary">
      <div>
        <strong>{descriptor.display_name}</strong>
        <span>{descriptor.category}</span>
      </div>
      <p>{descriptor.description}</p>
      <small>
        {configuredCount}/{descriptor.fields.length} parameters configured
      </small>
    </div>
  );
}

export function EditableFieldRow({
  label,
  value,
  valueKind,
  options,
  help,
  status = "stable",
  statusNote = null,
  saveState,
  onSave,
}: {
  label: string;
  value: string;
  valueKind: "string" | "number" | "enum" | "boolean";
  options?: string[];
  help?: string;
  status?: FieldStatus;
  statusNote?: string | null;
  saveState: SaveState;
  onSave: (value: string) => Promise<void>;
}) {
  const [draftValue, setDraftValue] = useState(value);
  const isDirty = draftValue !== value;

  useEffect(() => {
    setDraftValue(value);
  }, [value]);

  return (
    <div className={isDirty ? "parameter-row dirty" : "parameter-row"}>
      <div className="parameter-label">
        <strong>{label}</strong>
        <span>{valueKind}</span>
      </div>
      <FieldStatusBadge status={status} note={statusNote} />
      {help && <p className="parameter-help">{help}</p>}
      {valueKind === "enum" ? (
        <select value={draftValue} onChange={(event) => setDraftValue(event.target.value)}>
          {(options ?? []).map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
      ) : valueKind === "boolean" ? (
        <label className="parameter-toggle">
          <input
            type="checkbox"
            checked={draftValue === "true"}
            onChange={(event) => setDraftValue(String(event.target.checked))}
          />
          <span>{draftValue}</span>
        </label>
      ) : (
        <input
          value={draftValue}
          type={valueKind === "number" ? "number" : "text"}
          onChange={(event) => setDraftValue(event.target.value)}
        />
      )}
      <button
        className="parameter-save"
        disabled={!isDirty || saveState === "saving"}
        onClick={() => onSave(draftValue)}
      >
        {saveState === "saving" ? "Saving" : "Save"}
      </button>
    </div>
  );
}

function editableKindForParameter(
  parameter: GraphParameter,
  fieldSpec?: FieldSpec,
): "string" | "number" | "enum" | "boolean" {
  if (fieldSpec?.kind === "enum" && fieldSpec.options.length > 0) {
    return "enum";
  }

  if (fieldSpec?.kind === "boolean") {
    return "boolean";
  }

  if (fieldSpec?.kind === "integer" || fieldSpec?.kind === "number") {
    return "number";
  }

  if (parameter.value_kind === "boolean") {
    return "boolean";
  }

  if (parameter.value_kind === "number") {
    return "number";
  }

  return "string";
}

function selectOptionsForValue(fieldSpec: FieldSpec, value: string) {
  if (!value || fieldSpec.options.includes(value)) {
    return fieldSpec.options;
  }

  return [value, ...fieldSpec.options];
}

function validateRules(
  rules: JsonValue[],
  actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null,
) {
  const issues: ValidationIssue[] = [];

  if (rules.length === 0) {
    issues.push({ path: "rules", message: "At least one rule is required." });
  }

  rules.forEach((rule, ruleIndex) => {
    const rulePath = `rules.${ruleIndex}`;
    const ruleObject = isJsonObject(rule) ? rule : {};
    const condition = isJsonObject(ruleObject.condition) ? ruleObject.condition : {};
    const actions = Array.isArray(ruleObject.actions) ? ruleObject.actions : [];
    const elseActions = Array.isArray(ruleObject.else_actions) ? ruleObject.else_actions : [];
    const fieldPath = condition.field_path;
    const operation = condition.operation;
    const isUnconditional = operation === "always" || operation === "never";

    if (!isUnconditional && typeof fieldPath === "string" && fieldPath.trim().length > 0) {
      const pathIssue = validateFieldPath(fieldPath);
      if (pathIssue) {
        issues.push({
          path: `${rulePath}.condition.field_path`,
          message: `Rule ${ruleIndex + 1}: ${pathIssue}`,
        });
      }
    }

    if (!isUnconditional && (typeof fieldPath !== "string" || fieldPath.trim().length === 0)) {
      issues.push({
        path: `${rulePath}.condition.field_path`,
        message: `Rule ${ruleIndex + 1}: field path is required.`,
      });
    }

    if (typeof operation !== "string" || !conditionOperationOptions.includes(operation)) {
      issues.push({
        path: `${rulePath}.condition.operation`,
        message: `Rule ${ruleIndex + 1}: operation is not supported.`,
      });
    }

    if (["<", "<=", ">", ">="].includes(typeof operation === "string" ? operation : "")) {
      const conditionValue = condition.value;
      if (typeof conditionValue !== "number") {
        issues.push({
          path: `${rulePath}.condition.value`,
          message: `Rule ${ruleIndex + 1}: comparison value must be numeric.`,
        });
      }
    }

    if (actions.length === 0) {
      issues.push({
        path: `${rulePath}.actions`,
        message: `Rule ${ruleIndex + 1}: at least one action is required.`,
      });
    }

    validateRuleActions(actions, `${rulePath}.actions`, `Rule ${ruleIndex + 1} action`, actionSchema, issues);
    validateRuleActions(
      elseActions,
      `${rulePath}.else_actions`,
      `Rule ${ruleIndex + 1} else action`,
      actionSchema,
      issues,
    );
  });

  return issues;
}

function validateRuleActions(
  actions: JsonValue[],
  path: string,
  label: string,
  actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null,
  issues: ValidationIssue[],
) {
  actions.forEach((action, actionIndex) => {
    const actionPath = `${path}.${actionIndex}`;
    const actionObject = isJsonObject(action) ? action : {};
    const type = actionObject.type;
    const variant =
      typeof type === "string"
        ? actionSchema?.variants.find((candidate) => candidate.tag_value === type)
        : undefined;

    if (typeof type !== "string" || !variant) {
      issues.push({
        path: `${actionPath}.type`,
        message: `${label} ${actionIndex + 1}: action type is not supported.`,
      });
      return;
    }

    variant.fields.forEach((field) => {
      const value = actionObject[field.key];
      if (!field.required || !isEmptyRequiredValue(value)) {
        validateRuleFieldValue(value, field, `${actionPath}.${field.key}`, `${label} ${actionIndex + 1}`, issues);
        return;
      }

      issues.push({
        path: `${actionPath}.${field.key}`,
        message: `${label} ${actionIndex + 1}: ${field.label.toLowerCase()} is required.`,
      });
    });

    if (
      type === "copy_field" &&
      typeof actionObject.source_field === "string" &&
      actionObject.source_field === actionObject.target_field
    ) {
      issues.push({
        path: `${actionPath}.target_field`,
        message: `${label} ${actionIndex + 1}: source and target must differ.`,
      });
    }

    if (
      type === "rename_field" &&
      typeof actionObject.old_field === "string" &&
      actionObject.old_field === actionObject.new_field
    ) {
      issues.push({
        path: `${actionPath}.new_field`,
        message: `${label} ${actionIndex + 1}: old and new fields must differ.`,
      });
    }
  });
}

function validateRuleFieldValue(
  value: JsonValue | undefined,
  field: FieldSpec,
  path: string,
  label: string,
  issues: ValidationIssue[],
) {
  if (value === undefined || value === null) {
    return;
  }

  if (field.kind === "array" && !Array.isArray(value)) {
    issues.push({
      path,
      message: `${label}: ${field.label.toLowerCase()} must be an array.`,
    });
    return;
  }

  if (field.kind === "object" && !isJsonObject(value)) {
    issues.push({
      path,
      message: `${label}: ${field.label.toLowerCase()} must be an object.`,
    });
  }

  if (field.key === "field_paths" && Array.isArray(value)) {
    for (const item of value) {
      if (typeof item !== "string" || item.trim().length === 0) {
        issues.push({
          path,
          message: `${label}: field paths must be non-empty strings.`,
        });
        return;
      }

      const pathIssue = validateFieldPath(item);
      if (pathIssue) {
        issues.push({
          path,
          message: `${label}: ${pathIssue}`,
        });
        return;
      }
    }
  }

  if (["field_path", "source_field", "target_field", "old_field", "new_field"].includes(field.key)) {
    if (typeof value === "string" && value.trim().length > 0) {
      const pathIssue = validateFieldPath(value);
      if (pathIssue) {
        issues.push({
          path,
          message: `${label}: ${pathIssue}`,
        });
      }
    }
  }
}

function validateFieldPath(fieldPath: string) {
  if (fieldPath.trim().length === 0) {
    return "field path is required.";
  }

  let index = 0;
  let key = "";
  let expectSegment = true;

  while (index < fieldPath.length) {
    const char = fieldPath[index];

    if (char === ".") {
      if (expectSegment) {
        return "field path contains an empty segment.";
      }
      key = "";
      expectSegment = true;
      index += 1;
      continue;
    }

    if (char === "[") {
      key = "";
      index += 1;
      const start = index;

      while (index < fieldPath.length && fieldPath[index] !== "]") {
        index += 1;
      }

      if (index >= fieldPath.length) {
        return "field path has an unclosed array index.";
      }

      const rawIndex = fieldPath.slice(start, index);
      if (!/^\d+$/.test(rawIndex)) {
        return "field path array indexes must be non-negative integers.";
      }

      expectSegment = false;
      index += 1;

      if (index < fieldPath.length && fieldPath[index] !== "." && fieldPath[index] !== "[") {
        return "field path has unexpected text after an array index.";
      }
      continue;
    }

    key += char;
    expectSegment = false;
    index += 1;
  }

  if (expectSegment) {
    return "field path cannot end with a dot.";
  }

  return null;
}

function isEmptyRequiredValue(value: JsonValue | undefined) {
  if (value === undefined || value === null) {
    return true;
  }

  if (typeof value === "string") {
    return value.trim().length === 0;
  }

  return false;
}

function issueForPath(issues: ValidationIssue[], path: string) {
  return issues.find((issue) => issue.path === path || issue.path.endsWith(`.${path}`));
}

function isJsonObject(value: JsonValue): value is { [key: string]: JsonValue } {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function formatJsonValue(value: JsonValue) {
  if (value === null) {
    return "null";
  }

  if (typeof value === "string") {
    return value;
  }

  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  return JSON.stringify(value);
}

function parseJsonLikeValue(value: string): JsonValue {
  const trimmed = value.trim();

  if (trimmed.length === 0) {
    return "";
  }

  try {
    return JSON.parse(trimmed) as JsonValue;
  } catch {
    return value;
  }
}

function ruleActionSchema(schema: SchemaSpec): Extract<SchemaSpec, { kind: "tagged_union" }> | null {
  if (schema.kind !== "array" || schema.item.kind !== "object") {
    return null;
  }

  const actionsField = schema.item.fields.find((field) => field.key === "actions");
  if (actionsField?.schema?.kind === "array" && actionsField.schema.item.kind === "tagged_union") {
    return actionsField.schema.item;
  }

  return null;
}

function defaultActionForVariant(schema: Extract<SchemaSpec, { kind: "tagged_union" }>, tagValue: string) {
  const variant = schema.variants.find((candidate) => candidate.tag_value === tagValue);
  const action: { [key: string]: JsonValue } = { [schema.tag]: tagValue };

  variant?.fields.forEach((field) => {
    action[field.key] = defaultValueForField(field);
  });

  return action;
}

function defaultRule(actionSchema: Extract<SchemaSpec, { kind: "tagged_union" }> | null): JsonValue {
  return {
    condition: {
      field_path: "",
      operation: "always",
      value: null,
    },
    actions: actionSchema ? [defaultActionForVariant(actionSchema, actionSchema.variants[0]?.tag_value ?? "")] : [],
    else_actions: [],
  };
}

function moveArrayItem<T>(items: T[], index: number, direction: -1 | 1) {
  const nextIndex = index + direction;

  if (nextIndex < 0 || nextIndex >= items.length) {
    return items;
  }

  const nextItems = [...items];
  const [item] = nextItems.splice(index, 1);
  nextItems.splice(nextIndex, 0, item);

  return nextItems;
}

function defaultValueForField(field: FieldSpec): JsonValue {
  if (field.default_value !== null) {
    return parseJsonLikeValue(field.default_value);
  }

  if (field.kind === "boolean") {
    return false;
  }

  if (field.kind === "integer" || field.kind === "number") {
    return 0;
  }

  if (field.kind === "array") {
    return [];
  }

  if (field.kind === "object") {
    return {};
  }

  if (field.kind === "enum") {
    return field.options[0] ?? "";
  }

  return "";
}

function setObjectValue(
  value: { [key: string]: JsonValue },
  path: string[],
  nextValue: JsonValue,
): { [key: string]: JsonValue } {
  if (path.length === 0) {
    return value;
  }

  const [head, ...tail] = path;

  if (tail.length === 0) {
    return { ...value, [head]: nextValue };
  }

  const child = isJsonObject(value[head]) ? value[head] : {};

  return {
    ...value,
    [head]: setObjectValue(child, tail, nextValue),
  };
}

function ruleSummary(condition: { [key: string]: JsonValue }, actionCount: number, elseActionCount: number) {
  const operation = formatJsonValue(condition.operation ?? "matches");
  const fieldPath = formatJsonValue(condition.field_path ?? "condition");
  const actionLabel = actionCount === 1 ? "action" : "actions";
  const elseActionLabel = elseActionCount === 1 ? "else action" : "else actions";

  if (operation === "always") {
    return `Always, ${actionCount} ${actionLabel}, ${elseActionCount} ${elseActionLabel}`;
  }

  if (operation === "never") {
    return `Never, ${actionCount} ${actionLabel}, ${elseActionCount} ${elseActionLabel}`;
  }

  return `If ${fieldPath} ${operation}, ${actionCount} ${actionLabel}, ${elseActionCount} ${elseActionLabel}`;
}

function labelFromKey(key: string) {
  return key
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function summarizeNestedValue(value: JsonValue, schema: SchemaSpec) {
  if (schema.kind === "object" && isJsonObject(value)) {
    const condition = value.condition;
    const actions = value.actions;
    const elseActions = value.else_actions;

    if (isJsonObject(condition)) {
      const fieldPath = formatJsonValue(condition.field_path ?? null);
      const operation = formatJsonValue(condition.operation ?? null);
      const actionCount = Array.isArray(actions) ? actions.length : 0;
      const elseCount = Array.isArray(elseActions) ? elseActions.length : 0;
      return ruleSummary(
        { field_path: fieldPath, operation },
        actionCount,
        elseCount,
      );
    }
  }

  if (schema.kind === "tagged_union" && isJsonObject(value)) {
    return formatJsonValue(value[schema.tag] ?? null);
  }

  if (Array.isArray(value)) {
    return `${value.length} items`;
  }

  if (isJsonObject(value)) {
    return `${Object.keys(value).length} fields`;
  }

  return formatJsonValue(value);
}

