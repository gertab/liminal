// Hand-written mirror of the Rust types in shopfloor/sim (scenario.rs, engine.rs, event.rs, command.rs).

export type Vec2 = { x: number; y: number };

// --- Static layout (scenario.rs: Scenario) -----------------------------------------------------

export type SinkDef =
  | { name: string; type: "stdout" }
  | { name: string; type: "jsonl"; path: string; append: boolean };

export interface MachineDef {
  id: string;
  label: string;
  position: Vec2;
  size: Vec2;
  dock: Vec2;
  sensors: string[];
}

export interface MmDef {
  id: string;
  label: string;
  start: Vec2;
  start_at: string | null;
  start_heading_rad: number;
  max_speed_mps: number;
  radius_m: number;
  /** Ids of the actions this MM can perform. */
  actions: string[];
  sensors: string[];
}

export interface ActionDef {
  id: string;
  label: string;
  requires_machine: boolean;
  allowed_while_moving: boolean;
}

/** "floor" = a free-standing sensor at a fixed position; it is its own entity. */
export type HostKind = "machine" | "mobile_manipulator" | "floor";

export interface SensorDef {
  id: string;
  name: string;
  host: string;
  host_kind: HostKind;
  kind: string;
  label: string;
  unit: string;
  default: number;
  min: number;
  max: number;
  period_ms: number;
  noise_std: number;
  /** Fixed position of a floor sensor; null for mounted sensors. */
  position: Vec2 | null;
}

export interface Scenario {
  name: string;
  source_path: string | null;
  sim: { tick_ms: number; seed: number; dock_tolerance_m: number };
  floor: { width_m: number; height_m: number; grid_m: number };
  machines: MachineDef[];
  mobile_manipulators: MmDef[];
  sensors: SensorDef[];
  actions: ActionDef[];
  output: { position_interval_ms: number; sinks: SinkDef[] };
}

// --- Dynamic state (engine.rs: WorldState) ------------------------------------------------------

export interface Place {
  x: number;
  y: number;
  at: string | null;
}

export interface MotionView {
  command_id: number;
  target: Place;
  remaining_m: number;
  eta_ms: number;
}

export interface MmView {
  id: string;
  x: number;
  y: number;
  heading_rad: number;
  speed_mps: number;
  at: string | null;
  motion: MotionView | null;
  /** This MM's actions and whether each can be performed right now (rules live in the engine). */
  actions: ActionAvailability[];
}

export interface ActionAvailability {
  id: string;
  available: boolean;
  reason: string | null;
}

export interface SensorView {
  id: string;
  setpoint: number;
  noise_std: number;
  last_value: number | null;
  last_ms: number | null;
}

export interface WorldState {
  sim_ms: number;
  paused: boolean;
  mobile_manipulators: MmView[];
  sensors: SensorView[];
}

// --- Output events (event.rs: SimEvent) — the Liminal contract ----------------------------------

export type Origin = "ui" | "scene" | "api";

interface Envelope {
  v: number;
  seq: number;
  ts: number;
  sim_ms: number;
}

export type SimEvent = Envelope &
  (
    | { kind: "position"; entity: string; x: number; y: number; heading_rad: number; speed_mps: number }
    | {
        kind: "move_command";
        entity: string;
        command_id: number;
        origin: Origin;
        from: Place;
        to: Place;
        distance_m: number;
        eta_ms: number;
        replaces: number | null;
      }
    | { kind: "arrived"; entity: string; command_id: number; x: number; y: number; at: string | null }
    | {
        kind: "stopped";
        entity: string;
        command_id: number;
        x: number;
        y: number;
        at: string | null;
        origin: Origin;
      }
    | {
        kind: "sensor";
        sensor: string;
        entity: string;
        sensor_kind: string;
        value: number;
        unit: string;
        x: number;
        y: number;
      }
    | {
        kind: "sensor_set";
        sensor: string;
        entity: string;
        sensor_kind: string;
        setpoint: number;
        noise_std: number;
        previous_setpoint: number;
        origin: Origin;
      }
    | {
        kind: "action";
        entity: string;
        action: string;
        command_id: number;
        x: number;
        y: number;
        at: string | null;
        origin: Origin;
      }
    | { kind: "sim"; state: "started" | "paused" | "resumed" | "reset"; scenario: string }
  );

export type EventKind = SimEvent["kind"];

export const EVENT_KINDS: EventKind[] = [
  "position",
  "move_command",
  "arrived",
  "stopped",
  "sensor",
  "sensor_set",
  "action",
  "sim",
];

// --- Commands (command.rs) ---------------------------------------------------------------------

export type Target = { point: Vec2 } | { machine: string };

export type SimCommand =
  | { type: "move_to"; mm: string; target: Target }
  | { type: "stop"; mm: string }
  | { type: "set_sensor"; sensor: string; value: number }
  | { type: "set_sensor_noise"; sensor: string; std: number }
  | { type: "perform"; mm: string; action: string };

export type Control = "pause" | "resume" | "reset";
