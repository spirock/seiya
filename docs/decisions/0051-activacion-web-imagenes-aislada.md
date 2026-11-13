# ADR0051 · chat e imágenes con residencia alternada

El perfil permite arrancar el host local de imágenes con autorización acotada;
no una generación automática, descarga, instalación, cambio del servidor
compartido o relajación de memoria.

Se descarta el perfil solo imágenes antes de arrancarlo. Web temporal con chat Gemma e imágenes reales,
diseño existente,8015 y30min. Qwen apagado. Un slot
común ordena ambas modalidades, nunca dos residentes. Cada turno chat arranca
un runtime privado propio; tras respuesta confirma cleanup del árbol y libera
lease antes de admitir imagen. No adopta/descarga/mata servicios ajenos ni11434.
Petición de chat durante imagen espera como máximo el presupuesto del scheduler;
si no cabe, error visible, no respuesta simulada ni retry automático. La recarga
Gemma por turno añade latencia; historial browser se conserva y reenvía acotado.
Cada petición conserva inventario vacío, lease y guardas estrictas de ADR0050
para imágenes. Gemma conserva política elegida warning10s/swap2GiB, critical
inmediato y120s por turno. No se heredan excepciones W5.

Alternativa de residencia simultánea bloquea lease y ya observó critical; no se
elige. Cerrar runtime privado por turno evita depender de unload ambiguo. Permiso
vence a30min, bloquea nuevas peticiones; shutdown cancela y drena jobs antes de
salir. Si cleanup no confirma, mantener cuarentena, sin reinicio automático.

CLI opt-in separada exige aprobación y directorio privado nuevo. No cambia los
constructores/defaults anteriores ni la API image-jobs-v1. Estado anuncia modalidad
observada y espera, no concurrencia. Validación de arranque/chat
no acredita una imagen generada o calidad. Rollback: expirar/parar el host propio,
sin borrar modelos/evidencia/historial/configuración.

## Ampliación LAN autorizada

Usuario pide entrar desde otro PC de la red de casa. Bind a IP privada concreta
del Mac (no0.0.0.0), filtro de origen TCP a su subred IPv4 explícita, TLS1.3 con
certificado local y clave privada0600 fuera del repositorio. Host y Origin exactos
HTTPS, cookie Secure/HttpOnly/SameSite, bootstrap aleatorio de un uso. Sin UPnP,
port-forwarding, cambios de firewall, proxy confiado o exposición del runtime.
Certificado autofirmado necesita confianza manual en el otro PC; validar huella
por canal independiente. HTTP plano se rechaza; HTTPS mantiene crypto.randomUUID
disponible en navegador. Filtro IP no equivale a identidad: sigue exigiendo auth.
No acreditar acceso desde otro dispositivo hasta que el usuario lo pruebe.
