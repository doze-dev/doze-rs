// Before the run: one real doze-aws for every test (ports 41751 and 41752 unless the environment names others), checked to
// serve the bundle that is in web-dist. After the run it is stopped by its pid and its data directory removed.
import { assertCurrentBundle, start } from "./server";

export const LISTEN_PORT = Number(process.env.DOZE_AWS_E2E_PORT ?? 41751);
export const CONSOLE_PORT = Number(process.env.DOZE_AWS_E2E_CONSOLE_PORT ?? 41752);

export default async function globalSetup(): Promise<() => Promise<void>> {
  const server = await start(LISTEN_PORT, CONSOLE_PORT);
  try {
    await assertCurrentBundle(server.console);
  } catch (e) {
    await server.stop();
    throw e;
  }
  // Read by the tests (the workers inherit the environment).
  process.env.DOZE_AWS_E2E_ENDPOINT = server.endpoint;
  process.env.DOZE_AWS_E2E_CONSOLE = server.console;
  process.env.DOZE_AWS_E2E_DATA_DIR = server.dataDir;
  return () => server.stop();
}
