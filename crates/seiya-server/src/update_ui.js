// Explicit manual opt-in only; no check at page load and no automatic navigation.
function seiyaMountUpdate(base) {
  const target = document.getElementById('setup');
  if (!target || target.querySelector('#seiyaUpdateSection')) return;
  const section = document.createElement('section');
  section.id = 'seiyaUpdateSection';
  const title = document.createElement('h3');
  title.textContent = 'Actualizaciones';
  const note = document.createElement('p');
  note.textContent = 'Al comprobar, conectarás con GitHub, que recibirá tu dirección IP. No se descargará ni instalará nada.';
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = 'Comprobar actualizaciones';
  const status = document.createElement('p');
  status.setAttribute('role', 'status');
  button.addEventListener('click', async () => {
    button.disabled = true;
    status.textContent = 'Comprobando…';
    try {
      const response = await fetch(new URL('api/update-check', base), {
        method: 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error',
        headers: {'Content-Type': 'application/json'}, body: JSON.stringify({consent: true})
      });
      const data = await response.json();
      status.textContent = '';
      if (!response.ok) {
        status.textContent = data.error === 'update_check_rate_limited'
          ? 'Ya se comprobó; espera 24 horas para volver a intentarlo en esta sesión.'
          : 'No se pudo comprobar. Puedes seguir usando Seiya sin conexión.';
      } else if (!data.available) {
        status.textContent = 'No hay una versión estable más reciente.';
      } else if (typeof data.version === 'string' &&
                 /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(data.version) &&
                 data.release_url === 'https://github.com/spirock/seiya/releases/tag/v' + data.version) {
        const link = document.createElement('a');
        link.textContent = 'Ver versión ' + data.version + ' en GitHub';
        link.href = data.release_url;
        link.rel = 'noopener noreferrer';
        status.append(link);
      } else {
        status.textContent = 'Respuesta de actualización no válida.';
      }
    } catch (_) {
      status.textContent = 'No se pudo comprobar. Puedes seguir usando Seiya sin conexión.';
    } finally {
      button.disabled = false;
    }
  });
  section.append(title, note, button, status);
  target.append(section);
}
