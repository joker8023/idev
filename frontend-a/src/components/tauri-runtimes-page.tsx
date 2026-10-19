import { RuntimesPage } from "@multica/views/runtimes";
import { LOCAL_DAEMON_ID, LOCAL_MACHINE_NAME } from "../local-machine";

export function TauriRuntimesPage() {
  return (
    <RuntimesPage
      localDaemonId={LOCAL_DAEMON_ID}
      localMachineName={LOCAL_MACHINE_NAME}
      hasLocalMachine
      bootstrapping={false}
    />
  );
}
