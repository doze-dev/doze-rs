// Before the run: one real doze-aws for every test, checked to serve the bundle that is in web-dist. After the run it is
// stopped by its pid and its data directory removed.
//
// Ports: a run takes a block of 60 ports from 20000-28999 chosen by its process id, so two runs on one machine rarely meet; the
// environment can name others (DOZE_AWS_E2E_PORT, DOZE_AWS_E2E_CONSOLE_PORT). They stay below 32768 on purpose: Linux hands
// 32768-60999 to outgoing connections, and one of those can be holding a port a test wants (CI saw "address is in use").
// The block is written back to the environment so the test workers, started later, use the same ports.
import { assertCurrentBundle, start } from "./server";

const BLOCK = 60;
const block = 20_000 + (process.pid % 150) * BLOCK;
process.env.DOZE_AWS_E2E_PORT ??= String(block);
process.env.DOZE_AWS_E2E_CONSOLE_PORT ??= String(Number(process.env.DOZE_AWS_E2E_PORT) + 1);

export const LISTEN_PORT = Number(process.env.DOZE_AWS_E2E_PORT);
export const CONSOLE_PORT = Number(process.env.DOZE_AWS_E2E_CONSOLE_PORT);

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
