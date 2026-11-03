import { Pause, Play, RotateCcw } from "lucide-react";
import type { GraphNode } from "../../types";
import { ControlPanel } from "./ControlPanel";
import { formatSimTime } from "./format";
import { useShopfloorStage } from "./ShopfloorContext";

/**
 * Controls of a `shopfloor` input, shown in the inspector: simulation pause/reset, scenes, the
 * mobile manipulators (go to, actions, stop) and the sensors. They act on the running pipeline,
 * so they are disabled until it runs.
 */
export function ShopfloorInspector({ node }: { node: GraphNode }) {
  const shopfloor = useShopfloorStage(node);
  if (!shopfloor) {
    return null;
  }
  const { layout, world, error, running } = shopfloor;
  if (error) {
    return <p className="shopfloor-inspector-message is-error">{error}</p>;
  }
  if (!layout || !world) {
    return <p className="shopfloor-inspector-message">Loading scenario…</p>;
  }

  return (
    <div className="shopfloor-scope shopfloor-inspector">
      {!running && (
        <p className="sf-hint shopfloor-inspector-note">
          Run the pipeline to drive the shopfloor. Until then the floor shows its start positions.
        </p>
      )}
      {/* A disabled fieldset disables every button and input inside it. */}
      <fieldset className="shopfloor-controls" disabled={!running}>
        <div className="shopfloor-sim-row">
          <span className="shopfloor-sim-clock">{formatSimTime(world.sim_ms)}</span>
          {world.paused && <span className="sf-muted">paused</span>}
          <button
            type="button"
            className="chip-button"
            onClick={() => shopfloor.send({ control: world.paused ? "resume" : "pause" })}
            title={world.paused ? "Resume the simulation" : "Pause the simulation"}
          >
            {world.paused ? <Play size={11} /> : <Pause size={11} />}
            {world.paused ? "Resume" : "Pause"}
          </button>
          <button
            type="button"
            className="chip-button"
            onClick={() => shopfloor.send({ control: "reset" })}
            title="Back to start positions, default sensor values and time 0 (stops the scene)"
          >
            <RotateCcw size={11} />
            Reset
          </button>
        </div>
        <ControlPanel
          scenario={layout.scenario}
          world={world}
          selectedMm={shopfloor.selectedMm}
          focus={shopfloor.focus}
          lastActions={shopfloor.lastActions}
          onSelectMm={(id) => shopfloor.selectMm(shopfloor.selectedMm === id ? null : id)}
          onFocusSensor={(id) => shopfloor.setFocus({ type: "sensor", id })}
          send={(command) => shopfloor.send({ command })}
          onControl={(control) => shopfloor.send({ control })}
        />
      </fieldset>
    </div>
  );
}
