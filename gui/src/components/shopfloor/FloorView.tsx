import { useMemo, useRef, useState } from "react";
import type { MouseEvent } from "react";
import { formatValue, mmColor } from "./format";
import { clampToReach } from "./geometry";
import type { ActionEvent } from "./types";
import type { MachineDef, MmDef, MmView, Scenario, SensorDef, SensorView, Vec2, WorldState } from "./types";

/** Empty space drawn around the floor, in metres. */
const MARGIN_M = 0.9;
const LABEL_SIZE = 0.38;
const READOUT_SIZE = 0.28;
const FLOOR_SENSOR_LABEL_SIZE = 0.28;

/*
 * SVG text is never hit-tested here (`.floor-svg text { pointer-events: none }`). WebKit, the macOS
 * webview, puts the hit box of tiny viewBox-scaled text in the wrong place: some readings were not
 * clickable at all and others stole clicks from the empty floor next to them. Clickable text gets
 * an invisible `.hit-area` rect sized from the text instead.
 */
/** Advance of one character of the monospace readout font, in ems. */
const MONO_CHAR_EM = 0.6;
/** Rough average character width of the proportional label font, in ems. */
const SANS_CHAR_EM = 0.55;
const HIT_PAD = 0.08;

export type Focus = { type: "machine" | "sensor"; id: string } | null;

interface FloorViewProps {
  scenario: Scenario;
  world: WorldState | null;
  selectedMm: string | null;
  focus: Focus;
  lastActions: Record<string, ActionEvent>;
  onSelectMm: (id: string) => void;
  onFloorClick: (point: Vec2 | null) => void;
  onMachineClick: (id: string) => void;
  onSensorClick: (id: string) => void;
}

/**
 * The shopfloor, drawn in floor coordinates (metres). The SVG's y axis points down, so every y is
 * flipped with `sy()`; the floor model has its origin bottom-left with y up.
 */
export function FloorView({
  scenario,
  world,
  selectedMm,
  focus,
  lastActions,
  onSelectMm,
  onFloorClick,
  onMachineClick,
  onSensorClick,
}: FloorViewProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState<Vec2 | null>(null);
  const { width_m: width, height_m: height, grid_m: grid } = scenario.floor;
  const sy = (y: number) => height - y;

  const sensorDefs = useMemo(
    () => new Map(scenario.sensors.map((sensor) => [sensor.id, sensor])),
    [scenario.sensors],
  );
  const sensorViews = useMemo(
    () => new Map((world?.sensors ?? []).map((sensor) => [sensor.id, sensor])),
    [world?.sensors],
  );
  const mmViews = useMemo(
    () => new Map((world?.mobile_manipulators ?? []).map((mm) => [mm.id, mm])),
    [world?.mobile_manipulators],
  );
  const selectedDef = scenario.mobile_manipulators.find((mm) => mm.id === selectedMm) ?? null;

  /** Converts a mouse position to floor coordinates, or null when outside the floor. */
  const toFloor = (event: MouseEvent<SVGElement>): Vec2 | null => {
    const svg = svgRef.current;
    if (!svg) return null;
    const point = clientToViewBox(svg, event.clientX, event.clientY, {
      x: -MARGIN_M,
      y: -MARGIN_M,
      width: width + 2 * MARGIN_M,
      height: height + 2 * MARGIN_M,
    });
    const floorPoint = { x: point.x, y: height - point.y };
    const inside =
      floorPoint.x >= 0 && floorPoint.x <= width && floorPoint.y >= 0 && floorPoint.y <= height;
    return inside ? floorPoint : null;
  };

  const gridLines = useMemo(() => {
    const lines: JSX.Element[] = [];
    for (let x = grid; x < width - 1e-9; x += grid) {
      lines.push(<line key={`x${x}`} x1={x} y1={0} x2={x} y2={height} />);
    }
    for (let y = grid; y < height - 1e-9; y += grid) {
      lines.push(<line key={`y${y}`} x1={0} y1={height - y} x2={width} y2={height - y} />);
    }
    return lines;
  }, [width, height, grid]);

  const axisTicks = useMemo(() => {
    const step = width > 30 ? 10 : 5;
    const ticks: JSX.Element[] = [];
    for (let x = 0; x <= width + 1e-9; x += step) {
      ticks.push(
        <text key={`tx${x}`} x={x} y={height + 0.55} className="axis-label" textAnchor="middle">
          {x}
        </text>,
      );
    }
    for (let y = step; y <= height + 1e-9; y += step) {
      ticks.push(
        <text key={`ty${y}`} x={-0.25} y={height - y + 0.1} className="axis-label" textAnchor="end">
          {y}
        </text>,
      );
    }
    return ticks;
  }, [width, height]);

  const ghost = selectedDef && hover ? clampToReach(hover, selectedDef, width, height) : null;

  return (
    <svg
      ref={svgRef}
      className={`floor-svg${selectedMm ? " is-targeting" : ""}`}
      viewBox={`${-MARGIN_M} ${-MARGIN_M} ${width + 2 * MARGIN_M} ${height + 2 * MARGIN_M}`}
      preserveAspectRatio="xMidYMid meet"
      onClick={(event) => onFloorClick(toFloor(event))}
      onMouseMove={(event) => setHover(selectedMm ? toFloor(event) : null)}
      onMouseLeave={() => setHover(null)}
    >
      <rect className="floor" x={0} y={0} width={width} height={height} />
      <g className="floor-grid">{gridLines}</g>
      <g>{axisTicks}</g>

      {scenario.machines.map((machine) => (
        <Machine
          key={machine.id}
          machine={machine}
          sy={sy}
          focused={focus?.type === "machine" && focus.id === machine.id}
          targetable={selectedMm !== null}
          sensors={machine.sensors}
          sensorDefs={sensorDefs}
          sensorViews={sensorViews}
          focusedSensor={focus?.type === "sensor" ? focus.id : null}
          onClick={() => onMachineClick(machine.id)}
          onSensorClick={onSensorClick}
        />
      ))}

      {scenario.sensors
        .filter((sensor) => sensor.position !== null)
        .map((sensor) => (
          <FloorSensor
            key={sensor.id}
            def={sensor}
            view={sensorViews.get(sensor.id)}
            sy={sy}
            focused={focus?.type === "sensor" && focus.id === sensor.id}
            onClick={() => onSensorClick(sensor.id)}
          />
        ))}

      {scenario.mobile_manipulators.map((def, index) => {
        const view = mmViews.get(def.id);
        return view ? (
          <MobileManipulator
            key={def.id}
            def={def}
            view={view}
            color={mmColor(index)}
            sy={sy}
            selected={def.id === selectedMm}
            lastAction={lastActions[def.id]}
            sensorDefs={sensorDefs}
            sensorViews={sensorViews}
            focusedSensor={focus?.type === "sensor" ? focus.id : null}
            onClick={() => onSelectMm(def.id)}
            onSensorClick={onSensorClick}
          />
        ) : null;
      })}

      {ghost && selectedDef && (
        <circle className="ghost-target" cx={ghost.x} cy={sy(ghost.y)} r={selectedDef.radius_m} />
      )}
    </svg>
  );
}

/**
 * Mouse position (client pixels) → SVG user units (metres, y down), from the SVG's on-screen box
 * and its viewBox (`preserveAspectRatio="xMidYMid meet"`).
 *
 * Not `getScreenCTM()`: WebKit (the macOS webview) ignores CSS transforms on HTML ancestors there,
 * so inside the zoomed/panned canvas the target landed away from the mouse.
 */
function clientToViewBox(
  svg: SVGSVGElement,
  clientX: number,
  clientY: number,
  viewBox: { x: number; y: number; width: number; height: number },
): Vec2 {
  const rect = svg.getBoundingClientRect();
  const scale = Math.min(rect.width / viewBox.width, rect.height / viewBox.height);
  const left = rect.left + (rect.width - viewBox.width * scale) / 2;
  const top = rect.top + (rect.height - viewBox.height * scale) / 2;
  return { x: viewBox.x + (clientX - left) / scale, y: viewBox.y + (clientY - top) / scale };
}

interface SensorReadoutsProps {
  ids: string[];
  x: number;
  y: number;
  sensorDefs: Map<string, SensorDef>;
  sensorViews: Map<string, SensorView>;
  focusedSensor: string | null;
  onSensorClick: (id: string) => void;
}

/** Lines of "name value unit", starting at (x, y) in SVG coordinates and going down. */
function SensorReadouts({ ids, x, y, sensorDefs, sensorViews, focusedSensor, onSensorClick }: SensorReadoutsProps) {
  return (
    <>
      {ids.map((id, index) => {
        const def = sensorDefs.get(id);
        if (!def) return null;
        const view = sensorViews.get(id);
        const text = `${def.name} ${formatValue(view?.last_value ?? view?.setpoint)} ${def.unit}`;
        const baseline = y + index * (READOUT_SIZE + 0.08);
        const width = text.length * MONO_CHAR_EM * READOUT_SIZE + 2 * HIT_PAD;
        return (
          <g
            key={id}
            className={`sensor-readout-line${focusedSensor === id ? " is-focused" : ""}`}
            onClick={(event) => {
              event.stopPropagation();
              onSensorClick(id);
            }}
          >
            <rect
              className="hit-area"
              x={x - width / 2}
              y={baseline - READOUT_SIZE * 0.9 - 0.02}
              width={width}
              height={READOUT_SIZE * 1.15 + 0.04}
            />
            <text x={x} y={baseline} className="sensor-readout" textAnchor="middle">
              {text}
            </text>
          </g>
        );
      })}
    </>
  );
}

interface MachineProps {
  machine: MachineDef;
  sy: (y: number) => number;
  focused: boolean;
  targetable: boolean;
  sensors: string[];
  sensorDefs: Map<string, SensorDef>;
  sensorViews: Map<string, SensorView>;
  focusedSensor: string | null;
  onClick: () => void;
  onSensorClick: (id: string) => void;
}

function Machine({
  machine,
  sy,
  focused,
  targetable,
  sensors,
  sensorDefs,
  sensorViews,
  focusedSensor,
  onClick,
  onSensorClick,
}: MachineProps) {
  const { position, size, dock } = machine;
  const left = position.x - size.x / 2;
  const top = sy(position.y + size.y / 2);
  return (
    <g
      className={`machine${focused ? " is-focused" : ""}${targetable ? " is-targetable" : ""}`}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <title>{targetable ? `Send the selected MM to ${machine.label}` : machine.label}</title>
      <line className="dock-link" x1={position.x} y1={sy(position.y)} x2={dock.x} y2={sy(dock.y)} />
      <rect className="machine-body" x={left} y={top} width={size.x} height={size.y} rx={0.12} />
      <text x={position.x} y={top + LABEL_SIZE + 0.15} className="machine-label" textAnchor="middle">
        {machine.label}
      </text>
      <SensorReadouts
        ids={sensors}
        x={position.x}
        y={top + LABEL_SIZE + 0.2 + READOUT_SIZE + 0.1}
        sensorDefs={sensorDefs}
        sensorViews={sensorViews}
        focusedSensor={focusedSensor}
        onSensorClick={onSensorClick}
      />
      <circle className="dock-marker" cx={dock.x} cy={sy(dock.y)} r={0.14} />
    </g>
  );
}

interface FloorSensorProps {
  def: SensorDef;
  view: SensorView | undefined;
  sy: (y: number) => number;
  focused: boolean;
  onClick: () => void;
}

/** A free-standing sensor: a diamond marker with its label above and live value below. */
function FloorSensor({ def, view, sy, focused, onClick }: FloorSensorProps) {
  if (!def.position) return null;
  const cx = def.position.x;
  const cy = sy(def.position.y);
  const half = 0.2;
  const labelBaseline = cy - half - 0.12;
  const valueBaseline = cy + half + READOUT_SIZE + 0.05;
  const valueText = `${formatValue(view?.last_value ?? view?.setpoint)} ${def.unit}`;
  // One click target covering the label, the diamond and the value.
  const hitWidth =
    Math.max(
      def.label.length * SANS_CHAR_EM * FLOOR_SENSOR_LABEL_SIZE,
      valueText.length * MONO_CHAR_EM * READOUT_SIZE,
      2 * half,
    ) +
    2 * HIT_PAD;
  const hitTop = labelBaseline - FLOOR_SENSOR_LABEL_SIZE * 0.9 - HIT_PAD / 2;
  const hitBottom = valueBaseline + READOUT_SIZE * 0.25 + HIT_PAD / 2;
  return (
    <g
      className={`floor-sensor${focused ? " is-focused" : ""}`}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <title>{`${def.label} (${def.id}) at (${cx}, ${def.position.y})`}</title>
      <rect className="hit-area" x={cx - hitWidth / 2} y={hitTop} width={hitWidth} height={hitBottom - hitTop} />
      <path
        className="floor-sensor-marker"
        d={`M ${cx} ${cy - half} L ${cx + half} ${cy} L ${cx} ${cy + half} L ${cx - half} ${cy} Z`}
      />
      <text x={cx} y={labelBaseline} className="floor-sensor-label" textAnchor="middle">
        {def.label}
      </text>
      <text x={cx} y={valueBaseline} className="sensor-readout" textAnchor="middle">
        {valueText}
      </text>
    </g>
  );
}

interface MobileManipulatorProps {
  def: MmDef;
  view: MmView;
  color: string;
  sy: (y: number) => number;
  selected: boolean;
  lastAction: ActionEvent | undefined;
  sensorDefs: Map<string, SensorDef>;
  sensorViews: Map<string, SensorView>;
  focusedSensor: string | null;
  onClick: () => void;
  onSensorClick: (id: string) => void;
}

function MobileManipulator({
  def,
  view,
  color,
  sy,
  selected,
  lastAction,
  sensorDefs,
  sensorViews,
  focusedSensor,
  onClick,
  onSensorClick,
}: MobileManipulatorProps) {
  const r = def.radius_m;
  const cx = view.x;
  const cy = sy(view.y);
  // SVG rotation is clockwise with y down, so negate the (counter-clockwise) heading.
  const headingDeg = (-view.heading_rad * 180) / Math.PI;
  const target = view.motion?.target;
  return (
    <g className={`mm${selected ? " is-selected" : ""}`} style={{ color }}>
      {target && (
        <g className="mm-route">
          <line x1={cx} y1={cy} x2={target.x} y2={sy(target.y)} />
          <path
            d={`M ${target.x - 0.2} ${sy(target.y) - 0.2} L ${target.x + 0.2} ${sy(target.y) + 0.2} M ${target.x - 0.2} ${sy(target.y) + 0.2} L ${target.x + 0.2} ${sy(target.y) - 0.2}`}
          />
        </g>
      )}
      <g
        className="mm-body"
        onClick={(event) => {
          event.stopPropagation();
          onClick();
        }}
      >
        <title>{`${def.label} (${def.id}) — click to select`}</title>
        {selected && <circle className="mm-selection" cx={cx} cy={cy} r={r + 0.18} />}
        <circle className="mm-base" cx={cx} cy={cy} r={r} />
        <g transform={`translate(${cx} ${cy}) rotate(${headingDeg})`}>
          <path className="mm-heading" d={`M ${r * 0.15} ${-r * 0.45} L ${r * 0.95} 0 L ${r * 0.15} ${r * 0.45} Z`} />
        </g>
      </g>
      <text x={cx} y={cy + r + LABEL_SIZE + 0.05} className="mm-label" textAnchor="middle">
        {def.label}
      </text>
      {lastAction && (
        // Keyed by command id so the fade-out animation restarts for every new action.
        <text key={lastAction.command_id} x={cx} y={cy - r - 0.25} className="action-flash" textAnchor="middle">
          {lastAction.action}
        </text>
      )}
      <SensorReadouts
        ids={def.sensors}
        x={cx}
        y={cy + r + LABEL_SIZE + 0.15 + READOUT_SIZE}
        sensorDefs={sensorDefs}
        sensorViews={sensorViews}
        focusedSensor={focusedSensor}
        onSensorClick={onSensorClick}
      />
    </g>
  );
}
