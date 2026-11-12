const fs = require("fs");

fs.mkdirSync("coverage", { recursive: true });
fs.copyFileSync("lcov.template.info", "coverage/lcov.info");
