# ADR 0024 · lease nativo de sesión sin permiso implícito de carga

2026-10-26 · S4-02b.2, binding de lease macOS. Child/descendientes/observador real
siguen pendientes. No iniciar modelos ni usar la coordinación del servicio activo.

## Decisión antes de interfaces

- Consumir un Reservation ya adquirido por el host mediante into_session_lease.
  El wrapper nativo posee la reserva/file descriptor original; no acepta un número
  de FD, PID, comando o ruta del modelo. La conversión no libera puertos ni lock.
- Mantener separado ownership de procesos y lock. El supervisor solo llama a
  release tras ReapEvidence válida. Trait de lease aporta origen: los mocks siguen
  con snapshots versión1; un lease nativo añade lease_origin y versión2 de status.
  mode continúa injected-supervisor/model_inference false: runtime aún simulado.
- En release, verificar identidad dev/inode de directorio y archivo, FD original,
  archivo regular/sin symlink/hardlinks, UID efectivo y permisos privados de lock
  (sin bits de grupo/otros), directorio sin escritura de grupo/otros. Identidad
  capturada al transferir; un fallo inicial queda bloqueado, sin reset automático.
- Unlock mediante flock LOCK_UN del FD original, comprobando resultado; luego
  cerrar reserva/puertos. No unlink/truncate, shell o nuevas conexiones. Es síncrono
  en el primer poll, sin suspender tras unlock: cancellation-safe e idempotente.
  En error, conservar la reserva. No deserializar una confirmación del usuario/modelo.
- verify_binding permite comprobar condiciones antes de cargar; no concede carga
  ni sustituye autorización, presupuesto, preflight2 GiB, reserva o exclusividad.
- Pruebas usan directorios nuevos bajo target y el mismo nombre persistente del
  lock, nunca el directorio compartido operativo. Kernel flock real separado de
  runtime/reaping inyectados. Cualquier borrado de fixtures ocurre solo después
  de release/drop confirmados, nunca para liberar una reserva en uso.

## Alternativas, límites y migración

Desbloquear en Drop de un error, borrar el lockfile o confiar en health HTTP:
rechazado. Clonar FD al consumidor: rechazado. No modificar acquire/rearm/Drop de
campañas existentes; solo nuevo wrapper consumible y accessor interno de handle.

No es sandbox frente a otro proceso del mismo UID que renombre/reemplace ancestros
entre syscalls. Detecta cambios observados y retiene el lock del inode original,
pero no impide a un actor no cooperativo crear otro lockfile. Todos los coordinadores
deben respetar el mismo directorio/archivo persistente y nunca unlink. Requiere
filesystem local; syscalls síncronos no tienen deadline hard-real-time.

El resguardo fail-closed del supervisor conserva este wrapper en cuarentena y,
ante teardown/pánico, puede retener su FD hasta salida del proceso. No probar ese
último resguardo filtrando locks nativos: los tests de teardown siguen con fakes.
Los ensayos nativos nuevos se recuperan/cierran explícitamente y no dejan leases.
Sin bind nativo de runtime, modelo real, CLI/HTTP nuevo, bridge o cambios de Python.
