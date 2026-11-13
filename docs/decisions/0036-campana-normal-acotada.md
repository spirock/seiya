# ADR0036 · campaña normal con caducidad continua

2026-11-11, reanudación S3-05 autorizada. No cambia la excepción ADR0016: ABBA
sigue exigiendo gate normal≥2GiB antes de cada run, presión normal y lease.

Hallazgo: monitor_pressure comprobaba caducidad/cancelación continua solo para
calibración. La campaña normal podía continuar un listener/petición iniciados
antes de caducar. Decisión: verificar ensure_current en **todos** los ticks del
monitor, antes de leer presión o retornar por throttling. No extender permisos
por estar una petición en vuelo. Cleanup conserva handles/lease del runner.

Alternativas rechazadas: confiar solo en timeout HTTP (no es permiso), reutilizar
CalibrationApproval para12 runs (scope distinto), o bajar gate para completar el
sprint. No cronómetro hard-real-time: ticks/probes tienen latencia y cleanup
plazos propios. Al caducar deja de generar y cierra procesos propios, no ajenos.

Harness real nuevo ignored: aprobación normal de1800s, empieza antes de preparar
driver,12 runs ABBA,7 requests/run y≤256tokens; sin tools reales, red externa,
reintentos o parámetros de modelo inventados. Instrumentación de memoria igual
en ambos brazos, reportada como overhead de campaña, no ahorro físico probado.
Metadatos/fallos íntegros y resumen exploratorio por run; un gate fallido deja
campaña incompleta, no una baseline terminada. ModelLoadApproval sigue siendo
confirmación del operador, jamás un campo de respuesta/modelo que dé permiso.

API y receta sin cambios; corrección conservadora compatible. Tests de caducidad
y cancelación sin probes/modelos. Rollback del harness: no ejecutarlo; nunca
relajar la comprobación de caducidad para obtener cifras.
