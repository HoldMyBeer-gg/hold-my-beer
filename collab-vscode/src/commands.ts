import * as vscode from "vscode";
import type { CollabApi } from "./api";
import type { CollabResolvedConfig } from "./config";
import { CollabChatPanel } from "./chat";
export interface CommandDeps {
  getApi: () => CollabApi;
  getConfig: () => CollabResolvedConfig;
  outputChannel: vscode.OutputChannel;
  onOpenChat: (prefill?: string) => void;
}

function parseSendInput(text: string): { recipient: string; content: string } | null {
  const trimmed = text.trim();
  if (!trimmed) {
    return null;
  }
  const dm = /^@([\w-]+)\s+(.+)$/s.exec(trimmed);
  if (dm) {
    return { recipient: dm[1], content: dm[2].trim() };
  }
  return { recipient: "all", content: trimmed };
}

export function registerCollabCommands(
  context: vscode.ExtensionContext,
  deps: CommandDeps
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("collab.sendMessage", async () => {
      const cfg = deps.getConfig();
      if (!cfg.instance) {
        void vscode.window.showErrorMessage("Set collab.instance to send messages.");
        return;
      }
      const text = await vscode.window.showInputBox({
        prompt: "Message: @recipient text for DM, or plain text for broadcast",
        placeHolder: "@backend Deploy is ready",
      });
      if (text === undefined) {
        return;
      }
      const parsed = parseSendInput(text);
      if (!parsed || !parsed.content) {
        void vscode.window.showWarningMessage("Invalid message.");
        return;
      }
      try {
        const msg = await deps
          .getApi()
          .postMessage(cfg.instance, parsed.recipient, parsed.content, []);
        void vscode.window.showInformationMessage(`Sent (hash ${msg.hash.slice(0, 8)}…)`);
      } catch (e) {
        const err = e instanceof Error ? e.message : String(e);
        void vscode.window.showErrorMessage(`Send failed: ${err}`);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("collab.checkMessages", async () => {
      const cfg = deps.getConfig();
      if (!cfg.instance) {
        void vscode.window.showErrorMessage("Set collab.instance to check messages.");
        return;
      }
      deps.outputChannel.clear();
      deps.outputChannel.show(true);
      try {
        const list = await deps.getApi().getMessages(cfg.instance);
        deps.outputChannel.appendLine(`Messages for @${cfg.instance} (server filter: last 8h):`);
        deps.outputChannel.appendLine("");
        if (list.length === 0) {
          deps.outputChannel.appendLine("(no messages)");
          return;
        }
        for (const m of list) {
          deps.outputChannel.appendLine("─".repeat(40));
          deps.outputChannel.appendLine(`From: @${m.sender}  To: @${m.recipient}`);
          deps.outputChannel.appendLine(`Time: ${m.timestamp}`);
          deps.outputChannel.appendLine(`Hash: ${m.hash}`);
          deps.outputChannel.appendLine(m.content);
        }
      } catch (e) {
        const err = e instanceof Error ? e.message : String(e);
        deps.outputChannel.appendLine(`Error: ${err}`);
      }
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("collab.showRoster", async () => {
      await vscode.commands.executeCommand("workbench.view.extension.collab");
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("collab.openChat", (prefill?: string) => {
      deps.onOpenChat(typeof prefill === "string" ? prefill : undefined);
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("collab.showUsage", async () => {
      deps.outputChannel.clear();
      deps.outputChannel.show(true);
      try {
        const usage = await deps.getApi().getUsage();
        if (!usage.workers.length) {
          deps.outputChannel.appendLine("No usage data yet. Workers report to the server after each invocation.");
          return;
        }
        const pad = (s: string | number, n: number) => String(s).padEnd(n);
        const padL = (s: string | number, n: number) => String(s).padStart(n);
        const anyCost = usage.total_cost_usd > 0;

        deps.outputChannel.appendLine(
          `${pad("Worker", 20)} ${padL("Input", 10)} ${padL("Output", 10)} ${padL("Calls", 6)} ${padL("Secs", 8)}  ${pad("CLI", 10)} Tiers` + (anyCost ? "     Cost" : "")
        );
        deps.outputChannel.appendLine("─".repeat(anyCost ? 88 : 80));
        for (const w of usage.workers) {
          const tiers = `${w.full_calls}F/${w.light_calls}L`;
          const cost = anyCost ? `  $${w.cost_usd.toFixed(4)}` : "";
          deps.outputChannel.appendLine(
            `${pad(w.worker, 20)} ${padL(w.input_tokens, 10)} ${padL(w.output_tokens, 10)} ${padL(w.calls, 6)} ${padL(w.duration_secs, 8)}  ${pad(w.cli || "?", 10)} ${pad(tiers, 8)}${cost}`
          );
        }
        deps.outputChannel.appendLine("─".repeat(anyCost ? 88 : 80));
        const totalTiers = `${usage.total_full_calls}F/${usage.total_light_calls}L`;
        const totalCost = anyCost ? `  $${usage.total_cost_usd.toFixed(4)}` : "";
        deps.outputChannel.appendLine(
          `${pad("TOTAL", 20)} ${padL(usage.total_input_tokens, 10)} ${padL(usage.total_output_tokens, 10)} ${padL(usage.total_calls, 6)} ${padL(usage.total_duration_secs, 8)}  ${pad("", 10)} ${pad(totalTiers, 8)}${totalCost}`
        );
      } catch (e) {
        const err = e instanceof Error ? e.message : String(e);
        deps.outputChannel.appendLine(`Failed to fetch usage: ${err}`);
      }
    })
  );
}

export function openChatPanel(
  extensionUri: vscode.Uri,
  getApi: () => CollabApi,
  getInstance: () => string,
  prefill?: string
): void {
  CollabChatPanel.createOrShow(extensionUri, getApi, getInstance, prefill);
}
