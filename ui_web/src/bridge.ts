import { CoreEvent, ContextLevel } from "./types";

declare global {
  interface Window {
    BabyGervaiseBridge?: {
      postMessage: (payloadJson: string) => void;
    };
  }
}

export function sendCommand(command: string, payload: Record<string, unknown> = {}) {
  window.BabyGervaiseBridge?.postMessage(
    JSON.stringify({
      command,
      payload,
    }),
  );
}

export function listenToCoreEvents(handler: (event: CoreEvent) => void) {
  const listener = (event: Event) => {
    const customEvent = event as CustomEvent<CoreEvent>;
    handler(customEvent.detail);
  };

  window.addEventListener("baby-gervaise-event", listener as EventListener);
  return () => window.removeEventListener("baby-gervaise-event", listener as EventListener);
}

export function bootstrap() {
  sendCommand("bootstrap");
  sendCommand("request_overview");
}

export function requestOverview() {
  sendCommand("request_overview");
}

export function submitMessage(turnId: string, text: string) {
  sendCommand("send_message", {
    turnId,
    text,
    inputSource: "text",
  });
}

export function updateContextLevel(level: ContextLevel) {
  sendCommand("set_context_level", { level });
}
