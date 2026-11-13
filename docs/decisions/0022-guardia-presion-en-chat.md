# ADR 0022 · guardia stop-only de presión en chat Rust

2026-10-25 · S4-02b.1, integración con observaciones inyectadas, sin probes/modelos.
No completa S4-02b: ownership/parada/lease del runtime nativo será b.2.

## Decisión

- Reutilizar LoadMonitor de seiya-observe en una guardia de sesión compartida;
  seiya-server añade dependencia interna de workspace, sin paquetes externos.
  Mantener umbral de antigüedad2 s y gap5 s existentes. Reloj monotónico interno,
  inyectable para tests; regresión/futuro/desconocido/fallo bloquean.
- Guardia inicial awaiting_observation. Solo observaciones del host confiable,
  no deserializables de HTTP/modelos, pueden actualizarla. Una primera observación
  normal/fresca permite continuar los demás controles, NO autoriza una carga.
  El preflight2 GiB/reserva/clearance/presupuesto sigue siendo externo y obligatorio.
  No exigir2 GiB libres tras cada token: una carga consume páginas libres.
- Warning/critical/unknown/error/caducidad vuelven la guardia terminal; una muestra
  normal posterior no la rearma. Mismo monitor, sin política alternativa silenciosa.
- Scheduler::new_with_memory_guard es opt-in de librería y requiere adapter con
  runtime_status activado. Sin flag JSON de memoria/endpoint de actualización:
  falta productor nativo y ownership, no anunciar un supervisor operativo.
- Comprobar antes de admitir slot y de despachar, incluso tras esperar. Despertar
  esperas en cuanto haya stop. Vigilar caducidad cada250 ms mientras se espera o
  genera (best-effort; no hard-real-time). Sin tarea autónoma que sondee el host.
- Si stop llega durante generación, detener nuevas admisiones y mantener lease/
  slot hasta resultado o timeout del adapter. No matar proceso externo ni soltar
  GPU ficticiamente. Descartar éxito tardío y devolver error de memoria explícito;
  timeout/fallo ambiguo mantiene además cuarentena del adapter.
- Health con guardia usa runtime_status versión2 (sin guardia permanece versión1),
  con memory_admission estructurada versión1, scope stop_only_pressure,
  estado y causa. admission blocked prevalece, closed durante cierre. No decir
  que el modelo cabe o que su proceso fue limpiado. Modelo residente unknown.
- Default sin guardia conserva fixtures/wire de S4-02a y modo legacy. Errores
  nuevos503/code memory_observation_unavailable o runtime_memory_blocked, mensajes
  fijos, sin Retry-After, sin datos de procesos, consultas o rutas.

## Alternativas, alcance y migración

Duplicar parsers/umbrales en servidor o Python: rechazado. Confiar en un booleano
del cliente: rechazado. Cancelar el future y afirmar proceso parado: rechazado.
Usar esta guardia como permiso de carga o como prueba de2 GiB disponibles: rechazado.
Se añade solo un límite negativo, no presupuesto de asignación ni ahorro de RAM.

El adaptador HTTP existente no posee el runtime; su close_and_drain confirma el
trabajo del coordinador, no Child/descendientes/lease del backend. b.2 deberá unir
la guardia a un supervisor propietario y probar conservación de lease cuando el
cleanup no se confirme. No se sustituye por mocks que simplemente devuelvan true.
Sin binding Python/UI hasta S4-04. Sin activación CLI, campañas reales, procesos
ajenos, servicios del SO o modificación del OpenCode compartido.
