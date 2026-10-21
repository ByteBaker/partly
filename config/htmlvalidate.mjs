// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

export default {
  extends: ["html-validate:recommended"],
  // Keep explicit label IDs when JavaScript replaces controls with dropdowns.
  rules: { "no-redundant-for": "off" },
};
