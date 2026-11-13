# ADR0049 · prueba reducida de Gemma

El perfil reducido autorizado permite revisar/recuperar
cuarentena si comprobaciones lo permiten, web temporal con mismo diseño, sin
Qwen/imágenes, guardas conservadas. No se presenta como Seiya completo ni fallback
silencioso del par. No otro modelo, permisos de SO o intervención en11434.

## Recuperación auditada (sin modelos)

Nuevo comando explícito del operador. Abrir lease existente con O_NOFOLLOW y
flock exclusivo **solo para auditar/archivar**, sin bypass de carga. Leer marcador
regular y evidencia propia acotada, exigir PID/puerto/error coincidentes y raíz
del reporte dentro del temporal aprobado. Verificar ausencia de raíz/grupo y
puerto libre de cada runtime anterior registrado,11434 vacío y observación normal.
Si algo no puede comprobarse: no renombrar marcador ni cargar. No señalizar PIDs
externos o ajenos durante revisión. Tras comprobaciones y permiso humano actual,
archivar marcador con nombre único (no borrar evidencia) y guardar acta privada.
Esta decisión operativa no convierte cleanup04 en confirmado retroactivamente.
Lease retenida durante toda revisión; cualquier marcador nuevo vuelve a bloquear.

## Perfil solo Gemma

Entry point opt-in separado por selector/aprobación explícitos. Staging de un
único tag/digest, OLLAMA_MAX_LOADED_MODELS=1, runtime original privado, puerto y
version/listener probados. Preflight ADR0047, warning10s/swap2GiB previamente
elegidos, critical/unknown inmediatos, contexto solicitado4096/salida256,
sesión≤30min y cierre ADR0048. Sin registrar/ejecutar Qwen, jobs, imágenes o tools.

Reutilizar HTML/CSS/auth cookie del host existente. `/api/chat` en solo acepta
model/messages textuales sin sesión de jobs; `/api/jobs` no disponible. Historial
separado, health `jobs.enabled=false`, images false y modo solo explícito en
elementos de estado existentes. Respuestas reales del runtime; ninguna regla
simulada. Prueba real exigida: warmup+chat por el camino browser, no compilar.

Alternativas: reenviar a11434 elude propiedad/cuarentena; cargar el par repite el
riesgo excluido; reutilizar demo no satisface al usuario. Se elige reutilizar
supervisor/transporte/router Rust con modo cerrado, no otro frontend Python.
Rollback: expiración/cierre del host propio y lease/puerto/grupo revisados; UI
original, modelos, configuraciones, conversaciones y servicios compartidos intactos.
