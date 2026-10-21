// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

import js from "@eslint/js";
import globals from "globals";
import ts from "typescript-eslint";

export default [
  { ignores: ["target/**", ".venv/**", ".cache/**", "designs/**", "test-results/**"] },
  js.configs.recommended,
  ...ts.configs.recommended.map(config => ({ ...config, files: ["**/*.ts"] })),
  {
    files: ["**/*.{js,mjs,ts}"],
    rules: {
      "no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
    },
  },
  {
    files: ["sdk/node/**/*.{js,ts}", "examples/node/*.js", "tests/sdk_contract/node/*.mjs", "eslint.config.mjs"],
    languageOptions: { globals: globals.node },
  },
  {
    files: ["sdk/node/**/*.js", "examples/node/**/*.js"],
    languageOptions: { sourceType: "commonjs" },
  },
  {
    files: ["crates/partly-server/dashboard/*.js"],
    rules: { "no-alert": "error" },
    languageOptions: {
      sourceType: "script",
      globals: globals.browser,
    },
  },
  {
    files: ["crates/partly-server/dashboard/app.js"],
    languageOptions: { globals: { createDropdown: "readonly", closeDropdown: "readonly" } },
  },
  {
    files: ["**/*.ts"],
    rules: { "no-unused-vars": "off" },
  },
];
