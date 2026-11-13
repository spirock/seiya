# ADR 0010 · runner A/B simulado, antes del driver real

2026-10-19 · S3-04. Librería interna, no CLI operativa ni campaña real.

El runner controla orden ABBA × 3, fases, validación de presupuestos/cuerpos,
samples, artefactos y cierre mediante un driver async inyectado. En esta tarea
la única entrada es `launch_simulation`: rechaza drivers no declarados simulados.
No existe constructor que autorice cargar modelos. El driver real y su unión
con el launcher/probes requerirán validación y autorización de S3-05 antes de
medir; no se interpreta un mock exitoso como baseline real.

Los payloads/config se toman del plan versionado de baseline. En cada fase se
comprueba número de requests, cuerpo efectivo MLX, techo de salida, mensajes,
tools, respuesta/finish/usage y resultado sintético, sin ejecutar herramientas.
Para comparar runs, solo se normalizan IDs de tool calls en una copia analítica;
no se modifica el historial transmitido. Un desajuste aborta antes del otro brazo.

Directorio nuevo obligatorio; create_new en manifest/events/summary. No borrar
o sobrescribir archivos existentes. Eventos JSONL y artefactos totales acotados,
sin historial de snapshots ilimitado: solo referencia de cuerpos de un run.
Artefactos siempre mode simulation, no hashes/hardware/versiones reales inventados.
Datos rechazados no se vuelcan a logs como supuesto diagnóstico privado.

El handle de control cancela cooperativamente. La tarea propia conserva driver
y artefactos mientras drena la fase en vuelo; después solicita stop y registra
si la liberación quedó confirmada. Drop del consumidor no mata al runtime ni
inicia el otro brazo. Timeout de fase/start/stop produce fallo e incertidumbre;
no retries. La futura implementación real debe conservar sus propios leases
incluso si su future pierde al consumidor, como el adaptador de inferencia.

Un driver simulado no prueba cleanup de un modelo real, PID/puerto nativo o
generación residual. Abort/panic del proceso completo tampoco tiene garantía
de recuperación: dejar marcador parcial cuando se pueda, sin falsear éxito.
Metadatos de ownership de snapshots son comprobados estructuralmente, pero
su origen real debe acreditarlo el launcher/adaptador, no el archivo JSON.

Pruebas con driver/clock/samples sintéticos y carpetas propias nuevas bajo target;
sin procesos, red, modelos o shell. Ninguna dependencia externa nueva.
