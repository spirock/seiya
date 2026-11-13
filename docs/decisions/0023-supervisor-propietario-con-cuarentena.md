# ADR 0023 · supervisor propietario y lease retenido

2026-10-25 · S4-02b.2, núcleo ensayado por inyección. Adaptador nativo de runtime/
lease, pruebas de Child/OS y activación siguen pendientes. No inferencia real.

## Decisión antes de interfaces

- Módulo seiya-observe::session_supervisor, modo fijo injected-supervisor.
  Recibe capacidades Rust de runtime/lease ya propios; nunca PIDs, comandos,
  confirmaciones o permisos de HTTP/modelos. No crea procesos ni concede carga.
- Ownership es manifiesto validado de identidades fuertes de raíz y miembros
  propios, máximo32, sin PIDs repetidos. Cleanup exige evidencia de la misma raíz,
  exactamente esos miembros reaped y cobertura completa, sin descendientes no
  controlados. Una respuesta bool o PID distinto no puede liberar el lease.
  El productor sigue siendo confiable: validar datos no autentica pruebas del SO.
- Un actor retiene runtime y lease, alimenta MemoryGuard incluso en idle y respeta
  plazos de sesión/probe/cleanup. Drop del consumidor pide stop; no aborta el actor.
  Cancelación/plazo/fin del supervisor cierran guardia antes de limpiar recursos.
- Cleanup correcto de todos los miembros precede a release del lease. Si falla
  stop/proof/release o vence deadline, estado quarantined: capacidades retenidas,
  guardia terminal, ninguna nueva sesión/reconexión. No liberar ni matar ajenos.
- Solo una petición explícita del dueño puede repetir cleanup (canal acotado1,
  máximo3 intentos totales). No retry de inferencia. Si ya hubo prueba válida de
  reaping y solo falló release, no volver a señalar procesos: reintentar solo lease.
- En cuarentena, el actor queda esperando sin consumir CPU. Si se pierde todo
  consumidor, no cierra ni destruye recursos automáticamente. Puede mantener el
  lease hasta terminar el proceso supervisor; no prometer recuperación automática.
- Último resguardo RAII ante pánico/destrucción del executor: cerrar guardia y
  retener deliberadamente las capacidades sin ejecutar sus Drop no verificados.
  Esto pierde recuperación en ese caso y consume recursos hasta salida de proceso,
  pero evita desbloquear otra carga cuando pueden quedar huérfanos. No sustituye
  durabilidad/exclusión tras crash del proceso; no autorización para reiniciarlo.

## Alternativas y límites

Retornar Err y soltar lease en Drop: rechazado. Afirmar que canceló al cancelar un
future: rechazado. Reintentar cleanup en bucle: rechazado. Registro global de
PIDs con kill genérico: rechazado. Capacidades retenidas permiten futura recuperación
explícita; no son sandbox contra código Rust hostil del mismo proceso.

El modo es solo de inyección: no se implementa un OwnedRuntime nativo que prometa
control de descendientes sin disponer de sus handles. El binding futuro debe
derivar identidad/reaping del Child original y lock real; no traducir salud HTTP
a ReapEvidence ni crear una autorización porque exista este constructor.

## Pruebas y migración

Muestras, reloj, runtime, identidades y lease sintéticos; comprobar orden de efectos,
errores/timeout, identidad reutilizada, miembros omitidos, recursos retenidos,
cancelación/Drop y recuperación limitada. Integración con scheduler/guardia sin
modelos ni procesos del usuario. Sin flags de CLI/HTTP, wire nuevo OpenAI/nativo,
dependencias externas o servidor activo cambiado. La UI/bridge sigue S4-04/c.
