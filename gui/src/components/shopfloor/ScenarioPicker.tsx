import { invoke } from "@tauri-apps/api/core";
import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

interface ScenarioPickerProps {
  value: string;
  onChange: (path: string) => void;
}

/**
 * Editor for a shopfloor input's `scenario` parameter: the scenario files found in
 * `config/shopfloor/`, or any file via the native picker. The value is the path the pipeline
 * reads (repo-relative when the file is inside the repo).
 */
export function ScenarioPicker({ value, onChange }: ScenarioPickerProps) {
  const [scenarios, setScenarios] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string[]>("list_shopfloor_scenarios")
      .then(setScenarios)
      .catch((caught) => setError(String(caught)));
  }, []);

  const options = value && !scenarios.includes(value) ? [value, ...scenarios] : scenarios;

  return (
    <div className="scenario-picker">
      <select value={value} onChange={(event) => onChange(event.target.value)} title={value}>
        {!value && <option value="">Choose a scenario…</option>}
        {options.map((path) => (
          <option key={path} value={path}>
            {path}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="scenario-picker-browse"
        title="Choose a scenario file"
        onClick={() => {
          invoke<string | null>("pick_shopfloor_scenario")
            .then((path) => {
              if (path) onChange(path);
            })
            .catch((caught) => setError(String(caught)));
        }}
      >
        <FolderOpen size={14} />
        Browse…
      </button>
      {error && <p className="scenario-picker-error">{error}</p>}
    </div>
  );
}
