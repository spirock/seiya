// DOM-only contract: hide an empty solo panel, not historical jobs or errors.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const panel = {
  hidden: true,
  children: [],
  replaceChildren() { this.children = []; },
};
let renders = 0;
const context = {
  activeId: 'active',
  jobsPanel: panel,
  renderJobs(data) {
    renders++;
    panel.hidden = false;
    panel.children = data.jobs.map(j => j.job_id);
  },
};
vm.createContext(context);
vm.runInContext(fs.readFileSync('crates/seiya-server/src/solo_jobs_ui.js', 'utf8'), context);
context.renderJobs({jobs: []}, 'active', 'session');
assert.equal(panel.hidden, true);
assert.deepEqual(panel.children, []);
context.renderJobs({jobs: [{job_id: 'historic'}]}, 'active', 'session');
assert.equal(panel.hidden, false);
assert.deepEqual(panel.children, ['historic']);
context.renderJobs({jobs: []}, 'other', 'session');
assert.equal(panel.hidden, false);
assert.equal(renders, 3);
console.log('Solo jobs: empty panel hidden, historical jobs retained, other chat unchanged (DOM only)');
