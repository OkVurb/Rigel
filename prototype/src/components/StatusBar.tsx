import { branch, modelName, sessionCost, sessionTokens } from "../data";

export function StatusBar({
  installed,
  targets,
  view,
}: {
  installed: number;
  targets: number;
  view: "packages" | "discover" | "targets";
}) {
  return (
    <div className="status" role="status">
      <span>
        <b>{modelName}</b>
      </span>
      <span className="sep">│</span>
      <span>{branch}</span>
      <span className="sep">│</span>
      <span className="cost">${sessionCost.toFixed(2)}</span>
      <span>{sessionTokens.toLocaleString()} tok</span>
      <div className="right">
        <span>
          <span className="dot" />
          {installed} skills
        </span>
        <span className="sep">│</span>
        <span className={view === "targets" ? "cost" : undefined}>{targets} targets</span>
        <span className="sep">│</span>
        <span>cache 4.1 MB · 0 offline misses</span>
      </div>
    </div>
  );
}
