// Only the conversation-only view hides an empty jobs panel.
// Historical jobs and observation errors remain visible.
const seiyaSoloRenderJobs = renderJobs;
renderJobs = function(data, chatId, session) {
  seiyaSoloRenderJobs(data, chatId, session);
  if (chatId === activeId && Array.isArray(data.jobs) && data.jobs.length === 0) {
    jobsPanel.replaceChildren();
    jobsPanel.hidden = true;
  }
};
