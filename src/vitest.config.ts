import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["source/typescripts/**/*.test.ts"]
  }
});
