# ADR0035 · canal privado de memoria del runtime propio

2026-11-11. Continuación autorizada de instrumentación offline. Sin autorización
de cargar Gemma, importar MLX, ejecutar campañas reales o tocar servicios activos.

## Alternativas

- Endpoint HTTP de métricas: añade superficie/auth/puertos y riesgo de adoptar
  runtime ajeno. Rechazado para esta medición aislada.
- Archivos o stdout compartido con logs: contaminación, races y datos atrasados.
  Rechazado. No parsear logs del modelo como contadores.
- Socketpair anónimo heredado como stdin/stdout del Child original, con wrapper
  Python incluido en el binario observador: elegido. No hay dirección, listener
  de red, rutas temporales, shell o PID que puedan conceder ownership.

## Contrato y lifecycle

Opt-in por builder del CampaignDriver, apagado por defecto. No cambia recetas,
aprobaciones, lease, preflight, watchdog, HTTP o servicio operativo. Wrapper
ejecuta la misma entrada Python mediante runpy en el mismo proceso; **no exec
ni fork**. Conserva canal privado antes de redirigir logs stdout a stderr
(ya descartado por launcher) y stdin de aplicación a devnull. Un hilo daemon
recibe solo `sample\n`, invoca las tres lecturas del adaptador sobre el módulo
mlx.core **si ya fue importado** por la entrada, y emite JSON≤4096 bytes + LF.
El wrapper no importa MLX, carga modelos o configura el asignador por sí mismo.
Límite1024 peticiones por runtime. No cola/historial de muestras en Python.

El colector conserva Child original e identidad PID/inicio nativa; verifica
antes/después de cada lectura. Canal ligado a esos descriptores, no a un PID
externo, cabecera/modelo o dato de reporte. Secuencia exacta desde0 y reloj
intra/entre muestras monotónico. Frame acotado, EOF/JSON inválido/identidad/
timeout/cancelación son terminales: se cierra el canal, sin reintento con bytes
parciales. Deadline≤500ms y nunca posterior al presupuesto restante; la parada
del Child y lease siguen a cargo del runner existente. Cancelar un future no
debe dejar una lectura/hilo de colector bloqueados; socket no bloqueante Tokio.
No llamar getters MLX desde el watchdog. Un getter Python bloqueado queda dentro
del runtime propio y requiere cleanup supervisado, no liberar lease prematuramente.

Reportes separados en diagnostics de fase, con identidad original, fase y ventana
de reloj del colector; RSS/Snapshot permanecen intactos. Telemetría no acredita
presupuesto de categorías, memoria total Metal, inferencia o compatibilidad MLX.
El wrapper añade hilo/overhead y cambia stdio; invalida comparación con campaña
no instrumentada como trabajo idéntico sin diseñar ambos brazos equivalentes.

## Compatibilidad, provenance y rollback

API interna opt-in, macOS del driver actual. Rehearsal anterior hace exec: no es
compatible con este wrapper y debe rechazarse, no presentarse como instrumentada.
El código wrapper/adaptador se incluye literalmente (sin rutas de importación
externas) y se registra en metadata de artefactos para identificar lo ejecutado.
No prueba el launcher restringido sin fork, ni deadlines hard-real-time.
Rollback: omitir builder; ruta anterior sin canal ni wrapper. Sin instalación,
reemplazo del prototipo o migración de artefactos existentes.
