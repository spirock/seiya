# ADR0037 · evaluación R1 separada de baseline/calibración

2026-11-11. S3-06 autorizada: una carga existente,180s incluida preparación,
≤2 requests sintéticas≤256tokens, gate normal≥2GiB/presión/lease. No tools
ejecutadas, cambio de template para ocultar incompatibilidad o modelo promovido.

## Alternativas y decisión

No ejecutar ABBA12 runs con R1 (permiso y objetivo distintos), ni estirar la
calibración de1 petición a dos, ni reparar prosa/razonamiento como tool call.
Modo de runner separado `candidate-evaluation`, rehearsal `candidate-rehearsal`.
Builder explícito no crea permisos; launch_campaign/calibration rechazan ese
driver. Misma ruta propietaria/normal y caducidad continua ADR0036.

Una sola ejecución Rust: listener2s, texto marcador, propuesta read sintética,
shutdown. Por compatibilidad la segunda fase se llama `tool-roundtrip` en el
enum interno existente, pero metadata declara **proposal-only**, sin resultado
sintético posterior ni roundtrip ejecutado. Dos peticiones independientes:
un resultado semántico negativo no impide observar el segundo; un error de
transporte/ownership/usage/presión/plazo aborta a cleanup, nunca retry.

## Evidencia y veredicto

Snapshots/identidad/plazos/presupuestos validados igual que baseline. Checks
semánticos separados: texto exacto sin tool calls/finish stop; propuesta única
read/fixture.txt con argumentos JSON acotados y finish tool_calls. Prosa/tags no
se convierten en acciones. Los mensajes/razonamiento del candidato **no se
persisten**: solo check booleano, finish_reason permitido, uso y tiempo. Requests
son fixtures sintéticas; no contienen conversaciones o documentos personales.

`completed` del runner significa protocolo observado y cleanup confirmado,
no candidato aprobado. Assessment distingue incompleto, rechazado para este
perfil o superado el smoke de2 peticiones. No es calidad general, fiabilidad
de tools ni sesión sostenida. El template que ignora schemas sigue sin cambiar.
Una evaluación negativa puede cerrar la evaluación acotada, no validar R1 como
worker. Gate fallido no evalúa al modelo y mantiene la tarea bloqueada.

## Migración y rollback

API interna aditiva; manifiesto conserva protocolo y número efectivo de requests.
Formato baseline/recetas/HTTP/servicio operativo intactos. No extraer contenido
del pensamiento para mejorar un marcador. Rollback: no usar builder/harness de
candidato; los otros modos conservan fases/criterios originales. No modelo o
servicio reiniciado para preparar/probar este cambio.
