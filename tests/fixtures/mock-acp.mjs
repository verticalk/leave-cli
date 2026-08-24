import readline from "node:readline";

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
let sessionCounter = 0;
let permissionCounter = 0;
const turns = new Map();

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function result(id, value) {
  send({ jsonrpc: "2.0", id, result: value });
}

function update(sessionId, value) {
  send({ jsonrpc: "2.0", method: "session/update", params: { sessionId, update: value } });
}

function finishTurn(permissionId, response) {
  const turn = turns.get(String(permissionId));
  if (!turn) return;
  turns.delete(String(permissionId));
  const outcome = response?.result?.outcome;
  const selected = outcome?.outcome === "selected" ? outcome.optionId : "cancelled";
  update(turn.sessionId, {
    sessionUpdate: "tool_call_update",
    toolCallId: "fixture-tool",
    title: "Run workspace check",
    status: selected === "cancelled" ? "failed" : "completed"
  });
  update(turn.sessionId, {
    sessionUpdate: "agent_message_chunk",
    messageId: `answer-${permissionId}`,
    content: {
      type: "text",
      text: selected === "cancelled"
        ? "The protected operation was rejected."
        : "Permission received. The real ACP round trip is working."
    }
  });
  result(turn.promptId, { stopReason: "end_turn" });
}

input.on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }

  if (!message.method) {
    finishTurn(message.id, message);
    return;
  }

  switch (message.method) {
    case "initialize":
      result(message.id, {
        protocolVersion: 1,
        agentCapabilities: {
          sessionCapabilities: { resume: {} },
          promptCapabilities: {}
        },
        agentInfo: { name: "leave-acp-fixture", title: "Leave ACP Fixture", version: "1.0.0" }
      });
      break;
    case "session/new": {
      sessionCounter += 1;
      result(message.id, { sessionId: `fixture-session-${sessionCounter}` });
      break;
    }
    case "session/resume":
      result(message.id, {});
      break;
    case "session/prompt": {
      const sessionId = message.params.sessionId;
      permissionCounter += 1;
      const permissionId = `fixture-permission-${permissionCounter}`;
      update(sessionId, {
        sessionUpdate: "tool_call",
        toolCallId: "fixture-tool",
        title: "Run workspace check",
        kind: "execute",
        status: "pending"
      });
      turns.set(permissionId, { promptId: message.id, sessionId });
      send({
        jsonrpc: "2.0",
        id: permissionId,
        method: "session/request_permission",
        params: {
          sessionId,
          toolCall: {
            toolCallId: "fixture-tool",
            title: "Run workspace check",
            kind: "execute",
            status: "pending"
          },
          options: [
            { optionId: "allow-once", name: "Approve once", kind: "allow_once" },
            { optionId: "reject-once", name: "Reject", kind: "reject_once" }
          ]
        }
      });
      break;
    }
    case "session/cancel":
      break;
    default:
      if (message.id !== undefined) {
        send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "Method not found" } });
      }
  }
});
