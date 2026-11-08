# Roblox Studio

Rigel connects every native model to Roblox Studio through Studio's own MCP server. There is no
Rigel plugin to install and no extra model-provider account. The same server is added to a
project's `.mcp.json` for Chat-room harnesses when Studio is installed.

## What it can do

| Need | Tool |
|---|---|
| Read, search, create or edit a script | `script_read`, `script_search`, `script_grep`, `multi_edit` |
| Run Luau in Edit, Client or Server context | `execute_luau` |
| Generate a textured 3D mesh from a prompt | `generate_mesh` |
| Generate a material variant | `generate_material` |
| Generate a primitive-part model, optionally from a reference image | `generate_procedural_model` and `wait_job_finished` |
| Search the Creator Store or your inventory | `search_asset` |
| Insert a model, mesh, image, audio, video, animation or package by asset ID | `insert_asset` |
| Inspect or playtest the place | `get_studio_state`, `start_stop_play`, `get_console_output`, `screen_capture` |
| Make an original sound effect | `roblox_sound` |
| Make an animation | `execute_luau` with a `KeyframeSequence` |

Studio has no text-to-animation or text-to-audio generation tool. Rigel does not pretend it does:
animations are built as editable keyframes through Luau, and sound effects are synthesized
locally into WAV files. Meshes, materials and procedural models use Studio's own generators.

## Setup

1. Install the latest Roblox Studio and open a place.
2. In Studio, open **Assistant → … → Manage MCP Servers** and turn on **Enable Studio as MCP
   server**.
3. Windows normally uses `%LOCALAPPDATA%\Roblox\mcp.bat`; macOS uses
   `/Applications/RobloxStudio.app/Contents/MacOS/StudioMCP`. If Studio is elsewhere, set
   `RIGEL_ROBLOX_MCP` to the server path before starting Rigel.
4. In Rigel, call `roblox_studio` with no `tool`. It returns the live tool list and the open
   Studio windows. Keep the returned `studio_id` for later calls.

The Windows server is launched as `cmd.exe /c "cd /d %LOCALAPPDATA%\Roblox && .\mcp.bat"`,
from the server's own directory (or the parent of `RIGEL_ROBLOX_MCP`); starting it by absolute
path from the project leaves Studio's tool list empty. This is Studio's documented recipe, not a
shell command written by the model. Every call starts the local server, completes the MCP
handshake, asks for one tool, and stops it again.

## Skills

The [EL4CTEO/roblox-skills](https://github.com/EL4CTEO/roblox-skills) pack is 23 MIT-licensed
Roblox skills covering Luau, architecture, networking, security, data stores, monetization, UI,
audio, performance, testing and Studio MCP. Index it once:

```shell
rigel skills index EL4CTEO/roblox-skills
```

or press **Index Roblox skills** in **Settings → Skill shelf**. It is a normal GitHub skill
source: `skills_search` finds the matching `roblox-*` skill, `skill_load` pins its body hash and
returns it, and the Shelf can pin the ones a session should keep. The Roblox tool points the
agent at the pack before it writes Luau. The pack is not bundled and is not fetched unless you
index it.

## Generate a 3D asset

Ask the agent for the object, or call the tool directly:

```json
{
  "tool": "generate_mesh",
  "studio_id": "the id from list_roblox_studios",
  "arguments": { "prompt": "a weathered copper diving helmet, game-ready, muted teal patina" }
}
```

Use `generate_material` for a material variant on existing parts. Use
`generate_procedural_model` when the result should be made of editable Roblox primitives rather
than a mesh; follow it with `wait_job_finished` when Studio returns a job. `store_image` accepts
a local reference image for procedural generation.

Generation can take longer than a script edit, so the bridge allows up to three minutes. It
does not invent an asset ID: use the ID Studio returns, or search the Creator Store with
`search_asset` and insert that result with `insert_asset`.

## Generate an animation

Animations are Luau data, not an AI video. This creates an editable `KeyframeSequence` in
`ServerStorage` with poses for an R15 rig:

```json
{
  "tool": "execute_luau",
  "studio_id": "the id from list_roblox_studios",
  "arguments": {
    "datamodel_type": "Edit",
    "code": "local sequence = Instance.new('KeyframeSequence')\nsequence.Name = 'RigelWave'\nsequence.Loop = false\nfor i, degrees in ipairs({0, -45, 0}) do\n  local keyframe = Instance.new('Keyframe')\n  keyframe.Time = (i - 1) * 0.35\n  for _, joint in ipairs({'UpperTorso', 'LeftUpperArm'}) do\n    local pose = Instance.new('Pose')\n    pose.Name = joint\n    pose.CFrame = CFrame.Angles(0, 0, math.rad(degrees))\n    pose.Weight = 1\n    pose.Parent = keyframe\n  end\n  keyframe.Parent = sequence\nend\nsequence.Parent = game:GetService('ServerStorage')\nreturn sequence:GetFullName()"
  }
}
```

Pose names must match the rig's `Motor6D` joints. For R6, use names such as `Torso` and
`Left Arm`; for a custom rig, read the rig's joints first with `execute_luau`. Save the
`KeyframeSequence` to Roblox to obtain an animation asset ID, then set an `Animation` object's
`AnimationId` or insert the animation asset with `insert_asset`.

## Generate a sound effect

`roblox_sound` synthesizes PCM audio locally. It needs no key and makes no network request.

```json
{
  "name": "pickup-chime",
  "preset": "pickup",
  "duration_ms": 600,
  "pitch": 1.2,
  "seed": 7
}
```

Presets are `ambient`, `click`, `explosion`, `footstep`, `jump`, `laser`, `pickup` and
`whoosh`. The result is written under `.rigel/roblox/sounds/` and returned as a project-relative
path. Import that WAV in Studio's **Asset Manager**, then insert the resulting audio asset ID:

```json
{
  "tool": "insert_asset",
  "studio_id": "the id from list_roblox_studios",
  "arguments": { "asset_type": "Audio", "asset_id": 1234567890 }
}
```

The import is the one manual step: Studio MCP has no audio-upload tool. Rigel reports the file
path and the next call instead of claiming the sound is already in the place. If the place
belongs to a published experience, you can instead enable **Publish & manage assets** in Studio's
MCP Permissions, approve `.rigel/roblox/sounds` as an upload folder, and let the agent use
`open_cloud_invoke` to upload the WAV. Rigel does not assume those per-game permissions exist.

## Safety

Read-only Studio tools run directly. Everything that can change the place, spend a generation
entitlement or upload content is shown to the user with the exact tool and arguments before it
runs; a denial means nothing ran. `execute_luau` is arbitrary code in Studio, so it receives the
same approval as a write. Studio changes are not filesystem patches and are not rolled back by
Rigel's patch journal; use Studio's undo history or a saved place.

Subagents are read-only and do not receive either Roblox tool. Plan mode hides them as well.
