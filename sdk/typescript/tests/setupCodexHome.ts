import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach } from "@jest/globals";

const originalAstralHome = process.env.ASTRAL_HOME;
let currentAstralHome: string | undefined;

beforeEach(async () => {
  currentAstralHome = await fs.mkdtemp(path.join(os.tmpdir(), "astral-sdk-test-"));
  process.env.ASTRAL_HOME = currentAstralHome;
});

afterEach(async () => {
  const astralHomeToDelete = currentAstralHome;
  currentAstralHome = undefined;

  if (originalAstralHome === undefined) {
    delete process.env.ASTRAL_HOME;
  } else {
    process.env.ASTRAL_HOME = originalAstralHome;
  }

  if (astralHomeToDelete) {
    await fs.rm(astralHomeToDelete, { recursive: true, force: true });
  }
});
