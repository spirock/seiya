# ADR0044 · página de prueba real Qwen aislada

Estado: **planteamiento detenido antes de implementación/arranque**. Usuario
aclara después que quiere la experiencia completa con Gemma, auxiliares e imágenes,
no una página Qwen sola. Conservar este diseño como antecedente, no como interfaz
implementada o autorización para cargar el conjunto. Puerto8011 no se abrió.

2026-11-11. Usuario confirma **Autorizar página real de Qwen** en8011: hasta10min
y10 peticiones de inferencia totales incluida una comprobación sintética del
operador, una activa,≤120s/≤256tokens por respuesta, contexto solicitado4096,
think=false. Memoria informativa, servidor privado y cierre automático. Sin
Gemma, tools, jobs, descargas o cambios de Ollama activo; preview8010 intacta.
Riesgo de ralentización/bloqueo informado. No permiso para publicar o instalar.

## Diseño

Adaptador manual macOS en local-tools-tests reutiliza staging protegido/lease/
Popen/libproc/cleanup existentes; no sustituye launcher del producto. Mantiene
lease toda la ventana, comprueba compartido vacío antes de cada inferencia y no
adopta procesos ajenos. Puerto8011 debe estar libre. Runtime loopback efímero,
HOME/cwd/entorno privados, NO_CLOUD/NOPRUNE y pesos originales read-only heredados.

Reutiliza UI y guardas HTTP del servidor Python mediante un perfil interno
opcional de prueba. /v1/chat/completions rechazado en este perfil para no eludir
el control; solo /api/chat de texto user/assistant. Sin browser_chat/search ni
intermediación del modelo para herramientas. Sin jobs/reader. El perfil normal
y preview conservan comportamiento/API. Identidad pública Seiya/Pegassus.

Rust valida texto/roles/campos, construye petición nativa fija a Qwen y valida
respuesta/usage/finalización. Presupuesto de contexto **estimado**, sin truncar:
bytes UTF-8+128 por mensaje+reserva256+margin256≤4096. No tokenizador ni garantía
de ventana efectiva; rechazar contexto excesivo y pedir chat nuevo. Máximo salida
256, no aceptar length como éxito. Controles de tamaño/deadline previos al modelo.

Host cuenta≤10 peticiones enviadas, sin cola/retry; errores inciertos bloquean
sesión. Memoria observada sin cancelación por presión/swap, errores de métrica
como null. Ownership/deadline/desconexión de runtime invalidan nuevas inferencias;
watchdog externo cierra solo árbol propio, verificando puertos y lease al final.
Deadline de sesión600s, cada request≤120s o restante; best-effort, no OOM garantizado.
Al consumir10 envíos se deniegan más, no iniciar otro modelo o reiniciar la cuota.

UI anuncia chat real Qwen solo, límites y expiración; sin afirmar herramientas,
ejecución de código o paralelismo. Namespace localStorage único del ensayo,
sin leer/migrar/borrar chats reales o demo. Historial local sin cifrar como UI
existente; no guardar textos de usuario/respuestas en evidencia del operador.
Clave aleatoria en memoria precargada en UI de loopback, no logs/localStorage;
Host/Origin/auth estrictos y no proxies/redirects del cliente del runtime.

Alternativas: apuntar UI al Ollama compartido pierde ownership y límites de
sesión; sustituir demo8010 mezcla evidencia e historial; habilitar jobs con dos
modelos excede autorización. Se elige puerto nuevo y perfil interno explícito.

Rollback: cierre automático o señal solo al proceso del ensayo con cleanup;
omitir perfil opcional. No configuración/instalación o pesos a revertir. Pruebas
HTTP sintéticas no acreditan Safari visual; smoke real único no acredita sesión
sostenida, calidad, contexto lleno o Gemma+Qwen. Después de arrancar queda
intervención humana para probar la página; registrar esa validación pendiente.
