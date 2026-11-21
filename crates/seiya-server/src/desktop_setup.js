'use strict';
(() => {
  const el = id => document.getElementById(id);
  const fields = {llm_binary:'setupLlm', model_store:'setupStore', image_binary:'setupImageBinary', image_assets:'setupImageAssets'};
  let baseline = [], approval = null, busy = false, polling = false, runtimeAvailable = true;
  const defaultBrainOptions = Array.from(el('setupBrains').options || [], option => option.value);
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
    if (code === 'model_manager_unsupported') return 'El gestor de modelos está habilitado solo en Linux por ahora.';
    if (code === 'ollama_unreachable') return 'No se pudo conectar con Ollama en este equipo. Instálalo e inícialo manualmente; Seiya no lo ejecutará automáticamente.';
    if (code === 'invalid_model_id') return 'El ID del modelo contiene caracteres no admitidos.';
    if (code === 'ollama_pull_rejected') return 'Ollama rechazó la descarga. Comprueba el ID y la conexión a Internet.';
    if (code === 'ollama_pull_failed') return 'Ollama informó un error durante la descarga.';
    if (code === 'ollama_stream_error' || code === 'ollama_invalid_response') return 'Ollama devolvió una respuesta que Seiya no pudo leer.';
    if (code === 'ollama_delete_rejected') return 'Ollama no pudo eliminar ese modelo.';
    if (code === 'ollama_inventory_not_checked') return 'Aún no se ha consultado el almacén privado. Pulsa «Consultar modelos instalados» y vuelve a revisar la configuración.';
    if (code === 'ollama_model_not_installed') return 'El modelo no aparece en el último inventario privado. Comprueba el ID, selecciona uno de la lista o descarga el que necesitas.';
    if (code === 'ollama_managed_roles_unsupported') return 'El modo Ollama privado admite solo un Cerebro, sin trabajadores ni imágenes.';
    if (code === 'ollama_owned_port_busy') return 'El puerto privado de Seiya está ocupado. No se conectó ni adoptó el proceso que lo usa.';
    if (code === 'ollama_owned_start_timeout') return 'La instancia privada de Ollama no respondió a tiempo; Seiya la detuvo.';
    if (code === 'ollama_binary_invalid') return 'No se encontró un ejecutable Ollama válido en la ruta configurada.';
    if (code === 'ollama_owned_start_failed') return 'No se pudo iniciar la instancia privada de Ollama.';
    if (code === 'ollama_owned_stop_failed') return 'Seiya no pudo confirmar el cierre de su instancia privada; no se considera detenido.';
    if (code === 'ollama_private_store_invalid') return 'No se pudo preparar el almacén privado de modelos de Seiya.';
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
  el('setupForm').addEventListener('input', event => {
    if(event.target!==el('setupProbeId'))invalidate();
    el('setupProbeResult').textContent='';
  });
  el('setupProbe').addEventListener('click', () => action(async () => {
    const id=el('setupProbeId').value.trim()||el('setupBrain').value.trim();
    const out=el('setupProbeResult');
    if(!id){out.textContent='Indica un ID del catálogo.';return;}
    out.textContent='Consultando el inventario…';
    const labels={advertised:'El motor anuncia el modelo.',absent:'El motor respondió, pero no anuncia el modelo.',unreachable:'No se pudo conectar con el motor.',timeout:'El motor no respondió a tiempo.',inventory_error:'No se obtuvo un inventario válido.'};
    try{
      const v=await api('inventory',{id});
      out.textContent=id+': '+(labels[v.state]||'Resultado desconocido.')+' Inferencia no probada.';
    }catch(e){
      out.textContent=(e.message==='unknown_catalog_id'?'ID no registrado en el catálogo.':e.message==='local_catalog_required'?'Primero configura un catálogo HTTP local.':'No se pudo consultar el catálogo.')+' Inferencia no probada.';
    }
  }));
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
  let awaitingStop = false;
  el('setupResidency').addEventListener('click', () => action(async () => {
    const out = el('setupResidencyResult');
    out.textContent = 'Consultando el motor…';
    try {
      const {loaded} = await api('residency', {});
      if (!el('setupResidency').hidden) out.textContent = loaded === true ? 'Última consulta: modelo cargado en memoria.' : loaded === false ? 'Última consulta: modelo fuera de memoria.' : 'No se pudo confirmar si el modelo está cargado.';
    } catch (_) { out.textContent = 'Estado del modelo no disponible.'; }
  }));
  el('setupStop').addEventListener('click', () => action(async () => { await api('stop', {}); invalidate(); awaitingStop = true; message('Parada solicitada; esperando confirmación de cierre.'); }));
  el('setupQuit').addEventListener('click', () => action(async () => { await api('quit', {}); message('Cerrando Seiya. Puedes cerrar esta pestaña.'); }));
  let modelPull = false;
  function modelSearchLink() {
    const q = el('managerSearch').value.trim();
    el('managerSearchLink').href = 'https://ollama.com/search' + (q ? '?q=' + encodeURIComponent(q) : '');
  }
  el('managerSearch').addEventListener('input', modelSearchLink);
  async function refreshInstalled() {
    const value = await api('models');
    const select = el('managerInstalled');
    select.replaceChildren(new Option('—', ''));
    const brains = el('setupBrains');
    const options = new Map(defaultBrainOptions.map(name => [name, new Option(name, name)]));
    for (const model of value.models) {
      const option = new Option(model.name + ' · ' + formatModelSize(model.size), model.name);
      select.add(option);
      if (!options.has(model.name)) options.set(model.name, new Option(model.name, model.name));
    }
    brains.replaceChildren(...options.values());
    el('managerEmpty').hidden = value.models.length !== 0;
    el('managerProgress').textContent = value.models.length
      ? value.models.length + ' modelo(s) local(es). Selecciona uno para usarlo como Cerebro.'
      : 'El almacén privado está vacío.';
  }
  el('managerUse').addEventListener('click', () => {
    const name = el('managerInstalled').value;
    if (!name) { el('managerProgress').textContent = 'Consulta la lista y selecciona un modelo instalado.'; return; }
    el('setupBrain').value = name; invalidate();
    el('managerProgress').textContent = 'Cerebro seleccionado: ' + name + '. Revisa la configuración para continuar.';
  });
  el('managerSuggest').addEventListener('click', () => {
    if (modelPull) return;
    el('managerDownloads').open = true;
    el('managerModelName').value = 'qwen2.5:1.5b';
    el('managerPull').focus();
  });
  el('managerRefresh').addEventListener('click', () => action(refreshInstalled));
  function formatModelSize(bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) return 'tamaño desconocido';
    const units = ['B','KB','MB','GB','TB']; let value = bytes, unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
    return value.toFixed(unit ? 1 : 0) + ' ' + units[unit];
  }
  el('managerPull').addEventListener('click', async () => {
    if (modelPull) return;
    const name = el('managerModelName').value.trim();
    if (!name) { el('managerProgress').textContent = 'Escribe el ID del modelo que quieres descargar.'; return; }
    modelPull = true;
    el('managerPull').disabled = true;
    el('managerCancel').disabled = false;
    el('managerDelete').disabled = true;
    el('managerRefresh').disabled = true;
    el('managerInstalled').disabled = true;
    el('managerProgress').textContent = 'Iniciando descarga en Ollama…';
    try {
      await api('models/pull', {name});
      let progress;
      do {
        await new Promise(resolve => setTimeout(resolve, 500));
        progress = await api('models/progress');
        const amount = Number.isFinite(progress.completed) && Number.isFinite(progress.total) && progress.total > 0
          ? ' · ' + Math.min(100, Math.floor(progress.completed / progress.total * 100)) + '% (' + formatModelSize(progress.completed) + ' / ' + formatModelSize(progress.total) + ')'
          : '';
        el('managerProgress').textContent = progress.status + amount;
      } while (progress.active);
      if (progress.error) throw new Error(progress.error);
      await refreshInstalled();
      if (progress.status === 'Cancelación solicitada.') {
        el('managerProgress').textContent = 'Descarga cancelada. Puede que Ollama tarde un momento en liberar la transferencia.';
      }
    } catch (e) {
      el('managerProgress').textContent = explain(e.message) + ' No se inició inferencia.';
    } finally {
      modelPull = false;
      el('managerPull').disabled = false;
      el('managerCancel').disabled = true;
      el('managerDelete').disabled = false;
      el('managerRefresh').disabled = false;
      el('managerInstalled').disabled = false;
    }
  });
  el('managerCancel').addEventListener('click', async () => {
    if (!modelPull) return;
    el('managerCancel').disabled = true;
    try { await api('models/cancel', {}); }
    catch (e) { el('managerProgress').textContent = explain(e.message); }
  });
  el('managerDelete').addEventListener('click', () => action(async () => {
    const name = el('managerInstalled').value;
    if (!name) { el('managerProgress').textContent = 'Selecciona un modelo instalado.'; return; }
    if (!window.confirm('¿Eliminar ' + name + ' del almacén de Ollama? Esta acción borra los pesos locales.')) return;
    await api('models/delete', {name});
    el('managerProgress').textContent = name + ' eliminado del almacén local.';
    await refreshInstalled();
  }));
  // Original chat scripts were deliberately omitted. Never send chat pre-setup.
  el('prompt').disabled = true; el('send').disabled = true;
  el('form').addEventListener('submit', e => e.preventDefault());
  el('runtimeStatus').textContent = 'Configura Seiya antes de conversar.';
  async function refresh() {
    if (polling) return;
    polling = true;
    try {
      const v = await api('status');
      const phases = {unconfigured:'Sin configurar',saved:'Detenido',starting:'Arrancando',running:'En marcha',stopping:'Deteniéndose',blocked:'Bloqueado; cierre no confirmado'};
      const label = 'Motor de Seiya: ' + (phases[v.phase] || 'Estado desconocido') + '.';
      if (el('setupEngineState').textContent !== label) el('setupEngineState').textContent = label;
      el('setupResidency').hidden = v.phase !== 'running' || v.residency_supported !== true;
      if (el('setupResidency').hidden) el('setupResidencyResult').textContent = '';
      el('setupInventory').hidden = v.inventory_supported !== true;
      el('ollamaManager').hidden = v.model_manager_supported !== true;
      el('setupPlatform').textContent = 'Equipo: ' + v.platform + ' / ' + (v.architecture || 'arquitectura no informada') + (v.inventory_supported ? ' · catálogo HTTP local' : v.runtime_available ? ' · adaptador disponible' : ' · sin adaptador de ejecución');
      el('setupEnter').hidden = v.phase !== 'running';
      el('runtimeStatus').textContent = 'Seiya: ' + v.phase;
      if (awaitingStop && ['saved','unconfigured','blocked'].includes(v.phase)) {
        awaitingStop = false;
        if (v.phase !== 'blocked' && !v.error) message('Motores detenidos. Ya puedes revisar la configuración.');
      }
      if (v.error) message(explain(v.error));
    } catch (_) {
      el('setupEngineState').textContent = 'Motor de Seiya: estado desconocido.';
      el('setupResidency').hidden = true; el('setupResidencyResult').textContent = '';
      message('Seiya no está disponible o la sesión caducó. Abre la aplicación de nuevo; no se reenvían acciones.');
    }
    finally { polling = false; }
  }
  action(detect); refresh(); setInterval(refresh, 1500);
})();
