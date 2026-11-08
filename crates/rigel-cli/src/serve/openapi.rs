//! The machine-readable contract for `rigel serve`.
//!
//! It lives beside the routes rather than in a fixture so that a change to a limit or an
//! operation id is one edit. A client (Muse, or any generated connector) should be able to
//! build itself from this document alone.

use serde_json::{Value, json};

use super::{Api, MAX_PROMPT_CHARS, VERSION};

pub(super) fn document(api: &Api) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Rigel coordinator API",
            "version": VERSION,
            "description": "Rigel's coordinator and project workspace over loopback HTTP. Auth is the bearer token in $APPDATA/Rigel/api.json (or RIGEL_API_CONFIG). Workspace file paths are project-relative and resolved by the patch pipeline; with --full-access, /v1/fs/* also reaches any path this user can, and writes outside the project use compare-and-swap without a patch journal. The active workspace follows the project the desktop window has open. A turn is one model call sequence, billed to the session and enforced by its budget cap.",
        },
        "servers": [{ "url": api.base_url() }],
        "components": {
            "securitySchemes": {
                "bearer": { "type": "http", "scheme": "bearer" }
            },
            "schemas": {
                "Error": {
                    "type": "object",
                    "properties": {
                        "error": {
                            "type": "object",
                            "properties": {
                                "code": { "type": "string" },
                                "message": { "type": "string" }
                            },
                            "required": ["code", "message"]
                        }
                    }
                },
                "TurnRequest": {
                    "type": "object",
                    "required": ["prompt"],
                    "properties": {
                        "prompt": { "type": "string", "maxLength": MAX_PROMPT_CHARS },
                        "agent": { "type": "string", "default": "coordinator", "description": "The native agent role the turn runs as. Created on first use." },
                        "provider": { "type": "string", "description": "A provider id from GET /v1/providers. Defaults to RIGEL_PROVIDER, then omniroute." },
                        "model": { "type": "string", "description": "A model id from GET /v1/models. Defaults to the provider's own default." },
                        "effort": { "type": "string", "enum": ["low", "medium", "high"] },
                        "max_steps": { "type": "integer", "minimum": 1, "maximum": 1000 },
                        "approve": { "type": "boolean", "default": false, "description": "Answer this turn's command, network and plan cards with allow-once. Default deny." },
                        "dry_run": { "type": "boolean", "default": false, "description": "Price the first step and create nothing." }
                    }
                },
                "WriteRequest": {
                    "type": "object",
                    "required": ["path", "content"],
                    "properties": {
                        "path": { "type": "string", "description": "Project-relative, forward slashes." },
                        "content": { "type": "string" },
                        "base_hash": { "type": "string", "description": "The sha256 from a read. Required to overwrite an existing file." },
                        "session": { "type": "string" },
                        "agent": { "type": "string" }
                    }
                },
                "EditRequest": {
                    "type": "object",
                    "required": ["path", "old_string", "new_string", "base_hash"],
                    "properties": {
                        "path": { "type": "string" },
                        "old_string": { "type": "string", "description": "Must occur exactly once." },
                        "new_string": { "type": "string" },
                        "base_hash": { "type": "string", "description": "The sha256 the read returned; a stale value is a 409." },
                        "session": { "type": "string" },
                        "agent": { "type": "string" }
                    }
                },
                "CommandRequest": {
                    "type": "object",
                    "required": ["command"],
                    "properties": {
                        "command": { "type": "string" },
                        "cwd": { "type": "string", "description": "Project-relative directory." },
                        "timeout_secs": { "type": "integer" },
                        "approve": { "type": "boolean", "default": false, "description": "Allow-once. Default deny." },
                        "session": { "type": "string" },
                        "agent": { "type": "string" }
                    }
                }
            }
        },
        "security": [{ "bearer": [] }],
        "paths": {
            "/v1/health": {
                "get": {
                    "operationId": "health",
                    "summary": "Liveness and project identity",
                    "responses": { "200": { "description": "This process, its version and project" } }
                }
            },
            "/v1/providers": {
                "get": {
                    "operationId": "listProviders",
                    "summary": "Every provider row and whether it holds a credential",
                    "responses": { "200": { "description": "Providers with id, default_model and ready" } }
                }
            },
            "/v1/models": {
                "get": {
                    "operationId": "listModels",
                    "summary": "The merged model roster",
                    "parameters": [
                        { "name": "provider", "in": "query", "schema": { "type": "string" }, "description": "One provider's own roster" },
                        { "name": "current", "in": "query", "schema": { "type": "string" }, "description": "The provider whose rows lead (default omniroute)" },
                        { "name": "all", "in": "query", "schema": { "type": "boolean" }, "description": "Include rows that cannot answer a chat turn" },
                        { "name": "refresh", "in": "query", "schema": { "type": "boolean" }, "description": "Skip the 60-second cache" }
                    ],
                    "responses": { "200": { "description": "Models with provider, name and price" } }
                }
            },
            "/v1/models/probe": {
                "get": {
                    "operationId": "getModelProbeReport",
                    "summary": "Read the persisted roster-probe report",
                    "responses": {
                        "200": { "description": "The last merged probe report, or report: null before the first batch" },
                        "500": { "description": "The report file exists but is not valid JSON" }
                    }
                },
                "post": {
                    "operationId": "probeModels",
                    "summary": "Probe a batch of roster models with a real one-word completion",
                    "description": "Batches the merged roster. Advance offset until next_offset is null. The relay row and non-chat rows are skipped unless include_non_chat is set. Results are merged into model-probe.json in the API data directory after every batch.",
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "properties": {
                                "provider": { "type": "string", "description": "Probe one provider instead of the merged roster" },
                                "offset": { "type": "integer", "minimum": 0, "default": 0 },
                                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 25 },
                                "concurrency": { "type": "integer", "minimum": 1, "maximum": 8, "default": 4 },
                                "timeout_secs": { "type": "integer", "minimum": 5, "maximum": 120, "default": 30 },
                                "max_tokens": { "type": "integer", "minimum": 1, "maximum": 64, "default": 8 },
                                "prompt": { "type": "string", "maxLength": 500, "default": "Reply with the single word OK." },
                                "effort": { "type": "string", "enum": ["low", "medium", "high"], "default": "low" },
                                "include_non_chat": { "type": "boolean", "default": false },
                                "models": { "type": "array", "items": { "type": "string" }, "description": "Restrict this batch to exact model ids" }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "Batch results with next_offset, or null at the end of the roster" },
                        "400": { "description": "Invalid batch or filter" },
                        "502": { "description": "The roster could not be listed" }
                    }
                }
            },
            "/v1/sessions": {
                "get": {
                    "operationId": "listSessions",
                    "summary": "Sessions, newest first",
                    "responses": { "200": { "description": "Every session in this project" } }
                },
                "post": {
                    "operationId": "startSession",
                    "summary": "Start a session",
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "budget_cents": { "type": "integer", "minimum": 1 }
                            }
                        } } }
                    },
                    "responses": { "200": { "description": "The new session" } }
                }
            },
            "/v1/sessions/{id}": {
                "get": {
                    "operationId": "getSession",
                    "summary": "One session and its agents",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Session, agents and spend" }, "404": { "description": "No such session" } }
                }
            },
            "/v1/sessions/{id}/turns": {
                "get": {
                    "operationId": "sessionTranscript",
                    "summary": "The transcript, oldest first",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50 } },
                        { "name": "items", "in": "query", "schema": { "type": "boolean", "default": true } }
                    ],
                    "responses": { "200": { "description": "Turns with their items" }, "404": { "description": "No such session" } }
                },
                "post": {
                    "operationId": "runTurn",
                    "summary": "Run one turn through the coordinator",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/TurnRequest" } } }
                    },
                    "responses": {
                        "200": { "description": "The finished turn, its usage and the session spend" },
                        "400": { "description": "Bad prompt, provider or model" },
                        "402": { "description": "The session or daily budget is exhausted" },
                        "404": { "description": "No such session" },
                        "409": { "description": "The session has ended" },
                        "502": { "description": "The provider or the turn failed" }
                    }
                }
            },
            "/v1/turns/{id}": {
                "get": {
                    "operationId": "getTurn",
                    "summary": "One turn's row",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Status, usage and stop reason" }, "404": { "description": "No such turn" } }
                }
            },
            "/v1/turns/{id}/items": {
                "get": {
                    "operationId": "turnItems",
                    "summary": "One turn's transcript items",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "Messages, tool calls, patches and usage" }, "404": { "description": "No such turn" } }
                }
            },
            "/v1/turns/{id}/cancel": {
                "post": {
                    "operationId": "cancelTurn",
                    "summary": "Cancel a running turn",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                    "responses": { "200": { "description": "The cancellation was signalled" } }
                }
            },
            "/v1/workspace": {
                "get": {
                    "operationId": "workspaceInfo",
                    "summary": "The project this API is scoped to, and the session and agent a file call inherits",
                    "responses": { "200": { "description": "Project root, session, agent and limits" } }
                }
            },
            "/v1/workspace/files": {
                "get": {
                    "operationId": "listFiles",
                    "summary": "One directory of the project tree",
                    "parameters": [
                        { "name": "path", "in": "query", "schema": { "type": "string", "default": "." } },
                        { "name": "session", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": { "200": { "description": "Entries with kind and size" }, "404": { "description": "No such directory" } }
                }
            },
            "/v1/workspace/read": {
                "get": {
                    "operationId": "readFile",
                    "summary": "One UTF-8 file and the sha256 an edit must carry",
                    "parameters": [
                        { "name": "path", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "session", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "content, sha256 and line count" },
                        "404": { "description": "No such file" },
                        "413": { "description": "The file is larger than the read cap" }
                    }
                }
            },
            "/v1/workspace/write": {
                "post": {
                    "operationId": "writeFile",
                    "summary": "Create a file, or overwrite one with its current sha256",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/WriteRequest" } } }
                    },
                    "responses": {
                        "200": { "description": "The applied patch: patch_id, diff and result_hash" },
                        "409": { "description": "The file exists without base_hash, or the base is stale" }
                    }
                }
            },
            "/v1/workspace/edit": {
                "post": {
                    "operationId": "editFile",
                    "summary": "Replace one exact occurrence in a file, guarded by its sha256",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/EditRequest" } } }
                    },
                    "responses": {
                        "200": { "description": "The applied patch: patch_id, diff and result_hash" },
                        "409": { "description": "The base is stale" },
                        "422": { "description": "old_string is absent or ambiguous" }
                    }
                }
            },
            "/v1/workspace/command": {
                "post": {
                    "operationId": "runCommand",
                    "summary": "Run one shell command in the project, through the approval gate",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CommandRequest" } } }
                    },
                    "responses": {
                        "200": { "description": "The command outcome: exit_code, stdout, stderr" },
                        "403": { "description": "Denied by the approval gate" }
                    }
                }
            },
            "/v1/workspaces": {
                "get": {
                    "operationId": "listWorkspaces",
                    "summary": "The active workspace and the folders under ~/Rigel",
                    "responses": { "200": { "description": "current, plus name, path and current for each row" } }
                }
            },
            "/v1/workspaces/open": {
                "post": {
                    "operationId": "openWorkspace",
                    "summary": "Switch the API to another workspace at runtime",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["path"],
                            "properties": { "path": { "type": "string", "description": "An existing directory." } }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "The workspace now served" },
                        "400": { "description": "Not a directory" },
                        "500": { "description": "The session database could not be opened" }
                    }
                }
            },
            "/v1/fs/list": {
                "get": {
                    "operationId": "listAnyPath",
                    "summary": "One directory anywhere on the machine (requires --full-access)",
                    "parameters": [
                        { "name": "path", "in": "query", "schema": { "type": "string", "default": "." } }
                    ],
                    "responses": {
                        "200": { "description": "Entries with absolute paths" },
                        "403": { "description": "The server was not started with --full-access" }
                    }
                }
            },
            "/v1/fs/read": {
                "get": {
                    "operationId": "readAnyFile",
                    "summary": "One UTF-8 file anywhere on the machine (requires --full-access)",
                    "parameters": [
                        { "name": "path", "in": "query", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "content, sha256 and line count" },
                        "403": { "description": "The server was not started with --full-access" },
                        "404": { "description": "No such file" }
                    }
                }
            },
            "/v1/fs/write": {
                "post": {
                    "operationId": "writeAnyFile",
                    "summary": "Create or replace a file outside the project (requires --full-access)",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["path", "content"],
                            "properties": {
                                "path": { "type": "string", "description": "Absolute, or relative to the active workspace." },
                                "content": { "type": "string" },
                                "base_hash": { "type": "string", "description": "Required to replace an existing file." },
                                "create_dirs": { "type": "boolean", "default": false },
                                "session": { "type": "string" },
                                "agent": { "type": "string" }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "sha256, bytes, journaled and outside_project" },
                        "403": { "description": "The server was not started with --full-access" },
                        "409": { "description": "The file exists without base_hash, or the base is stale" }
                    }
                }
            },
            "/v1/fs/edit": {
                "post": {
                    "operationId": "editAnyFile",
                    "summary": "Replace one exact occurrence outside the project (requires --full-access)",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["path", "old_string", "new_string", "base_hash"],
                            "properties": {
                                "path": { "type": "string" },
                                "old_string": { "type": "string" },
                                "new_string": { "type": "string" },
                                "base_hash": { "type": "string" },
                                "session": { "type": "string" },
                                "agent": { "type": "string" }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "sha256, bytes and journaled" },
                        "403": { "description": "The server was not started with --full-access" },
                        "409": { "description": "The base is stale" },
                        "422": { "description": "old_string is absent or ambiguous" }
                    }
                }
            },
            "/v1/concord": {
                "get": {
                    "operationId": "concordInfo",
                    "summary": "Muse's Concord identity and bridge endpoints",
                    "responses": { "200": { "description": "agent_id, package and the tool routes" } }
                }
            },
            "/v1/concord/tools": {
                "get": {
                    "operationId": "concordTools",
                    "summary": "The live tools/list from Concord",
                    "responses": {
                        "200": { "description": "Concord's five work-state tools" },
                        "502": { "description": "Concord could not be started" }
                    }
                }
            },
            "/v1/concord/call": {
                "post": {
                    "operationId": "concordCall",
                    "summary": "Call one Concord tool as agent `muse`",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["tool"],
                            "properties": {
                                "tool": { "type": "string", "description": "start_work, inspect_work, update_work, transfer_work or finish_work" },
                                "arguments": { "type": "object", "description": "Passed through; agent_id defaults to muse and start_work gets the workspace as cwd." }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "Concord's result plus its concatenated text" },
                        "422": { "description": "Concord reported a tool error" },
                        "502": { "description": "Concord could not be started or timed out" }
                    }
                }
            },
            "/v1/concord/inbox": {
                "get": {
                    "operationId": "concordInbox",
                    "summary": "Muse's pending Concord messages, collected by the background poller",
                    "parameters": [
                        { "name": "since", "in": "query", "schema": { "type": "integer", "default": 0 }, "description": "The last seq Muse handled; only newer messages are returned." },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50, "maximum": 200 } }
                    ],
                    "responses": {
                        "200": { "description": "agent, messages as {seq, message}, retained, oldest, revision, checked_at, updated_at and error" }
                    }
                }
            },
            "/v1/concord/reply": {
                "post": {
                    "operationId": "concordReply",
                    "summary": "Reply to one cached Concord message as `muse`; builds the update_work call",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["message_id", "content"],
                            "properties": {
                                "message_id": { "type": "string", "description": "The messageId from /v1/concord/inbox." },
                                "content": { "type": "string", "maxLength": 16384, "description": "The reply body." },
                                "to_agent_id": { "type": "string", "description": "Defaults to the cached message's senderAgentId." },
                                "idempotency_key": { "type": "string", "maxLength": 128, "description": "Defaults to muse-reply-<message_id> so a retry cannot double-send." }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "The update_work result, plus message_id and to_agent_id" },
                        "422": { "description": "Concord reported a tool error" },
                        "502": { "description": "Concord could not be started or timed out" }
                    }
                }
            },
            "/v1/app": {
                "get": {
                    "operationId": "appState",
                    "summary": "Whether the Rigel window is open, and how much Swarm work is waiting",
                    "responses": {
                        "200": { "description": "open, project, updated_at, heartbeat_age_ms, swarm.pending, swarm.oldest and a fingerprint" }
                    }
                }
            },
            "/v1/events": {
                "get": {
                    "operationId": "waitForEvents",
                    "summary": "Long-poll until the window opens or closes, the project changes, a Swarm job arrives, or Concord mail arrives",
                    "parameters": [
                        { "name": "wait", "in": "query", "schema": { "type": "integer", "default": 25, "maximum": 60 }, "description": "Seconds to hold the request open." },
                        { "name": "since", "in": "query", "schema": { "type": "string" }, "description": "The previous response's event.fingerprint. Omit for an immediate snapshot." }
                    ],
                    "responses": {
                        "200": { "description": "changed, plus event: the same body /v1/app returns" }
                    }
                }
            },
            "/v1/swarm/jobs": {
                "get": {
                    "operationId": "listSwarmJobs",
                    "summary": "The Swarm prompts waiting for Muse, oldest first",
                    "responses": {
                        "200": { "description": "jobs, each with id, project, prompt, targets, lead and created_at" }
                    }
                }
            },
            "/v1/swarm/jobs/next": {
                "post": {
                    "operationId": "claimSwarmJob",
                    "summary": "Take the oldest Swarm prompt out of the inbox; honor its lead model",
                    "responses": {
                        "200": { "description": "job with prompt, targets and lead, or null when the inbox is empty" },
                        "500": { "description": "The inbox could not be read" }
                    }
                }
            },
            "/v1/swarm/jobs/{id}/complete": {
                "post": {
                    "operationId": "completeSwarmJob",
                    "summary": "Move a claimed Swarm job into the completed audit directory",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "pattern": "^[A-Za-z0-9_-]+$", "maxLength": 200 } }
                    ],
                    "responses": {
                        "200": { "description": "The job is completed; repeating the request is safe" },
                        "400": { "description": "The job id is unsafe" },
                        "404": { "description": "The job does not exist" },
                        "409": { "description": "The job is queued rather than claimed" }
                    }
                }
            },
            "/v1/swarm/jobs/{id}/release": {
                "post": {
                    "operationId": "releaseSwarmJob",
                    "summary": "Return a claimed Swarm job to the inbox",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "pattern": "^[A-Za-z0-9_-]+$", "maxLength": 200 } }
                    ],
                    "responses": {
                        "200": { "description": "The job is queued again; repeating the request is safe" },
                        "400": { "description": "The job id is unsafe" },
                        "404": { "description": "The job does not exist" },
                        "409": { "description": "The job is already completed" }
                    }
                }
            },
            "/v1/room/start": {
                "post": {
                    "operationId": "startRoom",
                    "summary": "Start the agent room for the active workspace if it is not already up",
                    "responses": {
                        "200": { "description": "The room is reachable" },
                        "502": { "description": "The room server could not be started" }
                    }
                }
            },
            "/v1/room/harnesses": {
                "get": {
                    "operationId": "listRoomHarnesses",
                    "summary": "Configured harnesses and whether each can be started",
                    "parameters": [
                        {
                            "name": "force",
                            "in": "query",
                            "required": false,
                            "description": "Bypass the 60-second readiness cache and probe the harnesses now",
                            "schema": { "type": "boolean" }
                        }
                    ],
                    "responses": { "200": { "description": "harnesses with name, label, api, ready, status, evidence, remediation and credential; status is usable, missing, missing_key, unauthenticated, probe_timeout, provider_blocked or probe_failed" } }
                }
            },
            "/v1/room/agents": {
                "get": {
                    "operationId": "listRoomAgents",
                    "summary": "The agents currently in the room; adopts or starts the room first",
                    "responses": {
                        "200": { "description": "agents" },
                        "502": { "description": "The room server could not be adopted or started" }
                    }
                }
            },
            "/v1/room/agents/start": {
                "post": {
                    "operationId": "startRoomAgents",
                    "summary": "Start all configured room agents, or the named subset",
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "properties": {
                                "names": { "type": "array", "items": { "type": "string", "maxLength": 40 }, "description": "Omit or empty to start every configured ready harness" }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "started, problems, project and the current agents" },
                        "400": { "description": "An invalid agent name" },
                        "502": { "description": "The room server could not be adopted or started" }
                    }
                }
            },
            "/v1/room/create": {
                "post": {
                    "operationId": "createRoom",
                    "summary": "Create or adopt the room and start configured agents in one call",
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "properties": {
                                "names": { "type": "array", "items": { "type": "string", "maxLength": 40 } }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "created, started, problems, project and the current agents" },
                        "400": { "description": "An invalid agent name" },
                        "502": { "description": "The room server could not be created or adopted" }
                    }
                }
            },
            "/v1/room/messages": {
                "get": {
                    "operationId": "readRoomMessages",
                    "summary": "Read the room's recent messages",
                    "parameters": [
                        { "name": "channel", "in": "query", "schema": { "type": "string" } },
                        { "name": "since_id", "in": "query", "schema": { "type": "integer", "default": 0 } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 50, "maximum": 200 } }
                    ],
                    "responses": {
                        "200": { "description": "messages" },
                        "502": { "description": "The room server could not be adopted or started" }
                    }
                }
            },
            "/v1/room/watchdog": {
                "get": {
                    "operationId": "roomWatchdogState",
                    "summary": "Stuck-agent watchdog state for the agents this process started",
                    "responses": { "200": { "description": "enabled, threshold, stuck agents and recent restarts" } }
                },
                "post": {
                    "operationId": "checkRoomWatchdog",
                    "summary": "Run one stuck-agent inspection and restart owned stuck harnesses",
                    "requestBody": {
                        "required": false,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "properties": {
                                "stuck_after_secs": { "type": "integer", "minimum": 1, "maximum": 86400, "description": "Defaults to the configured threshold (900 s)." }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "stuck, restarted and skipped with reasons" },
                        "400": { "description": "An invalid threshold" },
                        "502": { "description": "The room server could not be adopted or started" }
                    }
                }
            },
            "/v1/room/assign": {
                "post": {
                    "operationId": "assignRoomWork",
                    "summary": "Post Muse's split of a Swarm prompt as one message per agent",
                    "requestBody": {
                        "required": true,
                        "content": { "application/json": { "schema": {
                            "type": "object",
                            "required": ["assignments"],
                            "properties": {
                                "channel": { "type": "string", "default": "general" },
                                "assignments": {
                                    "type": "array",
                                    "minItems": 1,
                                    "maxItems": 12,
                                    "items": {
                                        "type": "object",
                                        "required": ["agent", "task"],
                                        "properties": {
                                            "agent": { "type": "string", "maxLength": 40 },
                                            "task": { "type": "string", "description": "The piece of the Swarm prompt this agent owns." },
                                            "files": { "type": "array", "items": { "type": "string" } },
                                            "spawn_subagents": { "type": "boolean", "default": true }
                                        }
                                    }
                                }
                            }
                        } } }
                    },
                    "responses": {
                        "200": { "description": "channel and the agents the assignments were sent to" },
                        "400": { "description": "A missing agent or task, or more than twelve rows" },
                        "502": { "description": "The room server is not running" }
                    }
                }
            }
        }
    })
}
