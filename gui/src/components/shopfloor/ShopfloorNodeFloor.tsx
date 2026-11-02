import type { GraphNode } from "../../types";
import { formatSimTime } from "./format";
import { FloorView } from "./FloorView";
import { clampToReach } from "./geometry";
import { useShopfloorStage } from "./ShopfloorContext";

/** Size of the floor area inside a shopfloor node (the node itself is wider than usual). */
export const SHOPFLOOR_FLOOR_HEIGHT = 300;

interface ShopfloorNodeFloorProps {
  node: GraphNode;
  onSelectNode: (nodeId: string) => void;
}

/**
 * The live floor drawn inside a `shopfloor` input node on the canvas. Clicking an MM selects it;
 * with an MM selected (and the pipeline running), clicking a machine or a point sends it there.
 * Any click also selects the node, so its controls show in the inspector.
 */
export function ShopfloorNodeFloor({ node, onSelectNode }: ShopfloorNodeFloorProps) {
  const shopfloor = useShopfloorStage(node);
  if (!shopfloor) {
    return null;
  }

  const { layout, world, error } = shopfloor;
  if (error || !layout || !world) {
    // Same size as the floor, so the node (and the canvas fit) doesn't jump once it loads.
    return (
      <div
        className={`shopfloor-node-floor shopfloor-node-message${error ? " is-error" : ""}`}
        style={{ height: SHOPFLOOR_FLOOR_HEIGHT + 24 }}
      >
        {error ?? "Loading scenario…"}
      </div>
    );
  }

  const { scenario } = layout;
  const selectedDef = scenario.mobile_manipulators.find((mm) => mm.id === shopfloor.selectedMm) ?? null;
  const scene = world.scene;
  const status = shopfloor.running
    ? [
        formatSimTime(world.sim_ms),
        world.paused ? "paused" : null,
        scene ? `scene: ${scene.label} #${scene.iteration}` : null,
        selectedDef ? `${selectedDef.label} selected: click a machine or the floor` : null,
      ]
    : [shopfloor.live ? "stopped: last frame" : "not running: start positions"];

  return (
    // `nodrag` / `nopan`: clicks and drags here interact with the floor, not the canvas.
    <div
      className="shopfloor-scope shopfloor-node-floor nodrag nopan"
      onClickCapture={() => onSelectNode(node.id)}
    >
      <div className="shopfloor-node-canvas" style={{ height: SHOPFLOOR_FLOOR_HEIGHT }}>
        <FloorView
          scenario={scenario}
          world={world}
          selectedMm={shopfloor.selectedMm}
          focus={shopfloor.focus}
          lastActions={{}}
          onSelectMm={(id) => shopfloor.selectMm(shopfloor.selectedMm === id ? null : id)}
          onFloorClick={(point) => {
            if (!point || !selectedDef) {
              shopfloor.selectMm(null);
              shopfloor.setFocus(null);
              return;
            }
            if (shopfloor.running) {
              const target = clampToReach(point, selectedDef, scenario.floor.width_m, scenario.floor.height_m);
              shopfloor.send({ command: { type: "move_to", mm: selectedDef.id, target: { point: target } } });
            }
          }}
          onMachineClick={(id) => {
            if (selectedDef && shopfloor.running) {
              shopfloor.send({ command: { type: "move_to", mm: selectedDef.id, target: { machine: id } } });
            } else {
              shopfloor.setFocus({ type: "machine", id });
            }
          }}
          onSensorClick={(id) => shopfloor.setFocus({ type: "sensor", id })}
        />
      </div>
      <div className="shopfloor-node-status">{status.filter(Boolean).join(" · ")}</div>
    </div>
  );
}
