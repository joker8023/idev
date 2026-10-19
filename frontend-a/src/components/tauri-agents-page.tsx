import { AgentsPage } from "@multica/views/agents";
import { LOCAL_DAEMON_ID, LOCAL_MACHINE_NAME } from "../local-machine";

export function TauriAgentsPage() {
  return (
    <AgentsPage
      localDaemonId={LOCAL_DAEMON_ID}
      localMachineName={LOCAL_MACHINE_NAME}
      hasLocalMachine
    />
  );
}
