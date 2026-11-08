/**
 * Roadmap 42: the parent turn's subagent graph.
 *
 * A node names its role and every prerequisite; the state is the same terminal state the
 * coordinator recorded. Edges are shown as `dependency -> node` so a blocked child is legible
 * without opening its transcript.
 */
import type { SubagentGraph } from "./api";

const STATE_LABEL: Record<string, string> = {
  pending: "waiting",
  running: "running",
  done: "done",
  failed: "failed",
  blocked: "blocked",
  cancelled: "cancelled",
};

export function SubagentGraphView({ graph }: { graph: SubagentGraph }) {
  if (graph.nodes.length === 0) return null;
  const edges = graph.nodes.flatMap((node) =>
    node.depends_on.map((dependency) => ({ from: dependency, to: node.id })),
  );
  return (
    <section className="subagent-graph" aria-label={`Subagent graph for turn ${graph.parent_turn_id}`}>
      <div className="subagent-graph-head">
        <span className="mono">subagent graph</span>
        <span className="fine">{graph.nodes.length} node{graph.nodes.length === 1 ? "" : "s"}</span>
      </div>
      <ul className="subagent-graph-nodes">
        {graph.nodes.map((node) => (
          <li key={node.id} data-state={node.state}>
            <span className="mono subagent-node-id">{node.id}</span>
            <span className="fine">{node.role}</span>
            <span className={`subagent-state ${node.state}`}>{STATE_LABEL[node.state] ?? node.state}</span>
            {node.depends_on.length > 0 ? (
              <span className="fine subagent-depends">after {node.depends_on.join(", ")}</span>
            ) : (
              <span className="fine subagent-depends">root</span>
            )}
            {node.detail ? <span className="fine subagent-detail">{node.detail}</span> : null}
          </li>
        ))}
      </ul>
      {edges.length > 0 ? (
        <p className="fine subagent-edges">
          {edges.map((edge) => (
            <span key={`${edge.from}->${edge.to}`} className="mono subagent-edge">
              {edge.from} → {edge.to}
            </span>
          ))}
        </p>
      ) : null}
    </section>
  );
}
