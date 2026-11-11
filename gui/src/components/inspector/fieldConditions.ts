import { FieldCondition, FieldSpec, GraphParameter, JsonValue } from "../../types";

/** Looks up the current value of a sibling field, as a string (`null` when unset with no default). */
export type FieldValues = (key: string) => string | null;

function defaultOf(key: string, fields: FieldSpec[]): string | null {
  return fields.find((field) => field.key === key)?.default_value ?? null;
}

/** A node's field values: its parameters, or each field's default when it isn't set. */
export function parameterValues(parameters: GraphParameter[], fields: FieldSpec[]): FieldValues {
  return (key) => {
    const parameter = parameters.find((candidate) => candidate.key === key);
    if (!parameter) {
      return defaultOf(key, fields);
    }
    return typeof parameter.raw_value === "string" ? parameter.raw_value : parameter.value;
  };
}

/** The field values of one object in a list (e.g. one aggregate of a window). */
export function objectValues(item: { [key: string]: JsonValue }, fields: FieldSpec[]): FieldValues {
  return (key) => {
    const value = item[key];
    if (value === undefined || value === null) {
      return defaultOf(key, fields);
    }
    return typeof value === "string" ? value : JSON.stringify(value);
  };
}

function conditionHolds(condition: FieldCondition, values: FieldValues) {
  const value = values(condition.field);
  return value !== null && condition.values.includes(value);
}

/** Whether a field applies, given its siblings' values (see `FieldSpec.applies_when`). */
export function fieldApplies(field: FieldSpec, values: FieldValues): boolean {
  return (field.applies_when ?? []).every((condition) => conditionHolds(condition, values));
}

/** Why a field doesn't apply, e.g. "kind is last_n (used with tumbling or sliding)". */
export function inapplicableReason(field: FieldSpec, values: FieldValues): string {
  return (field.applies_when ?? [])
    .filter((condition) => !conditionHolds(condition, values))
    .map(
      (condition) =>
        `${condition.field} is ${values(condition.field) ?? "not set"} (used with ${condition.values.join(" or ")})`,
    )
    .join("; ");
}
