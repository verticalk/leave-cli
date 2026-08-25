import { describe, expect, it } from "vitest";
import { buildTimeline } from "./session-screen";
import type { LocalEvent } from "../types";

function event(sequence: number, kind: string, payload: Record<string, unknown> = {}): LocalEvent {
  return {
    sequence,
    eventId: `e${sequence}`,
    kind,
    occurredAtUnixMs: 1_700_000_000_000 + sequence * 1000,
    payload,
    sessionId: "session-1",
    workspaceId: "workspace-1"
  };
}

function chunk(sequence: number, sessionUpdate: string, text: string, messageId?: string): LocalEvent {
  return event(sequence, "session_update", {
    update: { sessionUpdate, content: { type: "text", text }, ...(messageId ? { messageId } : {}) }
  });
}

describe("buildTimeline", () => {
  it("keeps one streamed reply in one bubble even when Devin omits message ids", () => {
    const timeline = buildTimeline([
      event(1, "user_prompt", { text: "which ai model are you??" }),
      chunk(2, "agent_thought_chunk", "The user is asking "),
      chunk(3, "agent_thought_chunk", "which AI model I am."),
      chunk(4, "agent_message_chunk", "I'm Devin, "),
      chunk(5, "agent_message_chunk", "an AI agent built by Cognition.")
    ]);
    expect(timeline.map((message) => message.kind)).toEqual(["user", "thought", "agent"]);
    expect(timeline[1].body).toBe("The user is asking which AI model I am.");
    expect(timeline[2].body).toBe("I'm Devin, an AI agent built by Cognition.");
  });

  it("still groups by message id when Devin provides one", () => {
    const timeline = buildTimeline([
      chunk(1, "agent_message_chunk", "Hello ", "m1"),
      event(2, "session_update", { update: { sessionUpdate: "tool_call", toolCallId: "t1", title: "Read file" } }),
      chunk(3, "agent_message_chunk", "world", "m1")
    ]);
    expect(timeline.map((message) => message.kind)).toEqual(["agent", "tool"]);
    expect(timeline[0].body).toBe("Hello world");
  });

  it("does not merge replies across turns", () => {
    const timeline = buildTimeline([
      event(1, "user_prompt", { text: "first" }),
      chunk(2, "agent_message_chunk", "answer one "),
      chunk(3, "agent_message_chunk", "part two"),
      event(4, "prompt_completed"),
      event(5, "user_prompt", { text: "second" }),
      chunk(6, "agent_message_chunk", "answer two")
    ]);
    const agents = timeline.filter((message) => message.kind === "agent");
    expect(agents.map((message) => message.body)).toEqual(["answer one part two", "answer two"]);
  });

  it("merges same-kind segments while they stream contiguously", () => {
    const timeline = buildTimeline([
      chunk(1, "agent_thought_chunk", "thinking "),
      chunk(2, "agent_message_chunk", "speaking "),
      chunk(3, "agent_thought_chunk", "more thinking")
    ]);
    expect(timeline.map((message) => message.kind)).toEqual(["thought", "agent", "thought"]);
    expect(timeline[0].body).toBe("thinking ");
    expect(timeline[1].body).toBe("speaking ");
    expect(timeline[2].body).toBe("more thinking");
  });

  it("starts a new bubble when a tool call interrupts the stream", () => {
    const timeline = buildTimeline([
      chunk(1, "agent_message_chunk", "before the tool "),
      event(2, "session_update", { update: { sessionUpdate: "tool_call", toolCallId: "t1", title: "Read file" } }),
      chunk(3, "agent_message_chunk", "after the tool")
    ]);
    expect(timeline.map((message) => message.kind)).toEqual(["agent", "tool", "agent"]);
    expect(timeline[0].body).toBe("before the tool ");
    expect(timeline[2].body).toBe("after the tool");
  });
});
