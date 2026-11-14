'use strict';
(() => {
  const el = id => document.getElementById(id);
  const fields = {llm_binary:'setupLlm', model_store:'setupStore', image_binary:'setupImageBinary', image_assets:'setupImageAssets'};
  let baseline = [], approval = null, busy = false, polling = false, runtimeAvailable = true;
  const roleKey = 'seiya.desktop.roles.v1';
  function restoreRoles() {
    const roles = JSON.parse(localStorage.getItem(roleKey) || 'null');
    if (!roles) return false;
    if (typeof roles.brain !== 'string' || roles.brain.length > 200 || !Array.isArray(roles.workers) || roles.workers.length > 16 || !roles.workers.every(s=>typeof s==='string' && s.length<=200) || typeof roles.image !== 'string' || roles.image.length > 200) throw new Error('Borrador de roles inválido.');
    el('setupBrain').value = roles.brain; el('setupWorkers').value = roles.workers.join('\n'); el('setupImage').value = roles.image; invalidate();
    return true;
  }
  const message = text => { el('setupMessage').textContent = text; };
  function invalidate() { approval = null; el('setupApproval').hidden = true; }
  function draft() {
    const value = {};
    for (const [key, id] of Object.entries(fields)) value[key] = el(id).value;
    value.roles = {brain:el('setupBrain').value.trim(), workers:el('setupWorkers').value.split('\n').map(s=>s.trim()).filter(Boolean), image:el('setupImage').value.trim()};
    return value;
  }
  function fill(value) {
    for (const [key, id] of Object.entries(fields)) el(id).value = value[key] || '';
    el('setupBrain').value = value.roles.brain;
    el('setupWorkers').value = value.roles.workers.join('\n');
    el('setupImage').value = value.roles.image;
    invalidate();
  }
  async function api(path, body) {
    const options = {credentials:'same-origin', cache:'no-store'};
    if (body !== undefined) Object.assign(options, {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    const response = await fetch(new URL('api/' + path, location.href), options);
    const data = response.status === 202 && path === 'quit' ? {} : await response.json();
    if (!response.ok) throw new Error(data.error || 'Solicitud rechazada.');
    return data;
  }
  async function action(work) {
    if (busy) return;
    busy = true;
    for (const button of document.querySelectorAll('#setup button')) button.disabled = true;
    for (const input of document.querySelectorAll('#setup input, #setup textarea')) input.disabled = true;
    try { await work(); }
    catch (e) { invalidate(); message(explain(e.message) + ' No se reintentó la acción.'); }
    finally { busy = false; for (const button of document.querySelectorAll('#setup button')) button.disabled = false; for (const input of document.querySelectorAll('#setup input, #setup textarea')) input.disabled = false; el('setupStart').disabled = !approval?.executable; el('setupReview').disabled = !runtimeAvailable; }
  }
  function explain(code) {
    if (code === 'profile_directory_not_private' || code === 'private_directory_required') return 'La carpeta de Seiya no es privada. Revisa sus permisos; no se han cambiado automáticamente.';
    if (code === 'selected_roles_not_supported_by_runtime') return 'Las asignaciones elegidas aún no tienen un adaptador compatible. Puedes guardarlas sin iniciar.';
    if (code === 'profile_changed_no_overwrite') return 'El perfil cambió fuera de esta página. Detecta de nuevo y revisa antes de guardar.';
    if (code === 'stop_runtime_before_editing' || code === 'runtime_busy') return 'Detén los motores y espera la confirmación de cierre antes de cambiar la configuración.';
    if (code === 'operation_busy') return 'Hay otra operación en curso. Espera o solicita la parada.';
    if (code === 'platform_adapter_not_implemented') return 'El adaptador de inferencia de esta plataforma todavía no está implementado. Puedes guardar un borrador de los roles en el navegador.';
    if (code === 'cancelled') return 'Selección cancelada.';
    return code;
  }
  async function detect() {
    const v = await api('detect'); baseline = v.baseline; fill(v.draft);
    message(v.warning || 'Rutas detectadas. Revísalas; no se ha ejecutado ningún motor.');
    const status = await api('status');
    runtimeAvailable = status.runtime_available;
    if (!status.runtime_available) {
      el('setupReview').disabled = true;
      try {
        restoreRoles();
      } catch (_) { message('No se pudo recuperar el borrador de roles del navegador. No se ha activado ningún modelo.'); }
    }
  }
  el('setupRestoreRoles').addEventListener('click', () => action(async () => {
    message(restoreRoles() ? 'Borrador recuperado. Revisa antes de guardar el perfil o iniciar.' : 'No hay un borrador de roles guardado en este navegador.');
  }));
  el('setupSaveRoles').addEventListener('click', () => action(async () => {
    const roles = draft().roles;
    if (!roles.brain || roles.workers.length > 16 || [roles.brain,roles.image,...roles.workers].some(s=>s.length>200)) throw new Error('Límites de asignaciones excedidos.');
    localStorage.setItem(roleKey, JSON.stringify(roles));
    message('Asignaciones guardadas solo en este navegador, sin activar modelos. La configuración del motor se guarda por separado al confirmar.');
  }));
  el('setupForm').addEventListener('input', invalidate);
  el('setupDetect').addEventListener('click', () => action(detect));
  for (const button of document.querySelectorAll('[data-pick]')) button.addEventListener('click', () => action(async () => {
    const field = button.dataset.pick, v = await api('pick', {field}); el(fields[field]).value = v.path; invalidate();
  }));
  el('setupForm').addEventListener('submit', event => { event.preventDefault(); action(async () => {
    invalidate(); const v = await api('review', {draft:draft(),baseline}); approval = v;
    el('setupSummary').textContent = JSON.stringify(v.draft, null, 2);
    el('setupEffects').textContent = v.effects + (v.executable ? ' Compatibilidad de inferencia aún no probada aquí.' : ' Ejecución bloqueada para estas asignaciones/plataforma.');
    el('setupApproval').hidden = false;
    message('Revisión de rutas completada. Confirma los efectos antes de continuar.');
  }); });
  async function confirm(start) {
    if (!approval) return;
    const token = approval.approval; invalidate();
    const v = await api('confirm', {approval:token,start});
    message(start ? 'Preparando host. No se ha solicitado inferencia.' : 'Configuración guardada. Motores detenidos.');
    if (v.saved) { const current = await api('detect'); baseline = current.baseline; }
  }
  el('setupSave').addEventListener('click', () => action(() => confirm(false)));
  el('setupStart').addEventListener('click', () => action(() => confirm(true)));
  el('setupEnter').addEventListener('click', () => action(async () => {
    const v = await api('enter', {});
    if (v.url) {
      const target = new URL(v.url), current = new URL(location.href);
      if (target.origin !== current.origin || target.username || target.password || target.search || target.hash || !/^\/(session\/[a-f0-9]{64}|trial\/[a-f0-9]{32}\/)$/i.test(target.pathname)) throw new Error('Destino del chat rechazado.');
      location.assign(target.href);
    }
  }));
  el('setupStop').addEventListener('click', () => action(async () => { await api('stop', {}); invalidate(); message('Parada solicitada; esperando confirmación de cierre.'); }));
  el('setupQuit').addEventListener('click', () => action(async () => { await api('quit', {}); message('Cerrando Seiya. Puedes cerrar esta pestaña.'); }));
  // Original chat scripts were deliberately omitted. Never send chat pre-setup.
  el('prompt').disabled = true; el('send').disabled = true;
  el('form').addEventListener('submit', e => e.preventDefault());
  el('runtimeStatus').textContent = 'Configura Seiya antes de conversar.';
  async function refresh() {
    if (polling) return;
    polling = true;
    try {
      const v = await api('status');
      el('setupPlatform').textContent = 'Equipo: ' + v.platform + ' / ' + (v.architecture || 'arquitectura no informada') + (v.runtime_available ? ' · adaptador macOS disponible' : ' · inferencia local aún no implementada');
      el('setupEnter').hidden = v.phase !== 'running';
      el('runtimeStatus').textContent = 'Seiya: ' + v.phase;
      if (v.error) message(explain(v.error));
    } catch (_) { message('Seiya no está disponible o la sesión caducó. Abre la aplicación de nuevo; no se reenvían acciones.'); }
    finally { polling = false; }
  }
  action(detect); refresh(); setInterval(refresh, 1500);
})();
