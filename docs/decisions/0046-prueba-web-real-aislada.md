# ADR0046 · prueba web real aislada autorizada

Ensayo real acotado autorizado: Gemma+Qwen, página temporal, preflight/memoria y
parada por guardas. No otra imagen, modificación del servicio actual ni rediseño.

W7 se reabre para implementar/probar un host Rust/macOS opt-in. Un Ollama privado
original (versión esperada0.40.14), store privado con manifiestos fijados y links
de lectura a blobs existentes; sandbox niega escrituras al store original.
Sin descargar, instalar, adoptar servicio compartido, purge o fallback remoto.
Lease existente durante toda residencia y cleanup; Child/PID/inicio/grupo y
listener comprobados por libproc antes de enviar. Runtimes/API nativa Ollama y
OpenAI no se confunden: adaptador explícito acotado, thinking=false, contexto4096,
salida≤256tokens y timeout120s. Una instancia foreground y un auxiliar como máximo.

Preflight normal, páginas libres≥2GiB (no sumar cachés solapadas), disco≥4GiB,
Ollama compartido vacío y lease exclusiva. Eso permite un experimento, **no es un
presupuesto de producción validado**. Monitor cada250ms: presión distinta de1,
observación inválida, swap+512MiB, ownership perdido, log>4MiB o disco<2GiB cierran
gate y terminan solo árbol propio. Excepciones individuales ADR0040–43 y memoria
informativa de imagen **no se heredan**. No aflojar guardas o retry automático.
Sesión de prueba≤30min; status/cancel siguen disponibles al cerrar gate, pero no
se anuncian respuestas o modelos residentes si el ensayo se aborta.

Alternativas: usar11434 no acredita ownership; dos coordinadores sobre el mismo
runtime compartido violan exclusión; repetir el frontend Python contradice la
preferencia Rust; activar sin probar tool-use da una falsa integración. Se elige
router browser Rust preparado y host experimental explícito. No sustituye app
operativa ni API OpenCode. Imagen web sigue no disponible.

Autenticación del host temporal: clave aleatoria en memoria y acceso browser
efímero por bootstrap de un uso/cookie HttpOnly SameSite=Strict. Sin secretos en
HTML, localStorage, configuración versionada o logs HTTP; enlace privado solo al
usuario. Se conserva Host/Origin antes de bootstrap y API. UI/CSS no cambian.

Rollback: cerrar listener temporal, detener árbol propio, verificar grupo/puerto
vacíos y liberar lease; conservar evidencia privada y pesos/config/historial.
Si falla preflight o guardas, registrar bloqueo concreto sin declarar web real
validada. No segunda carga automática: nueva decisión/recursos para otro intento.
