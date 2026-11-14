# ADR0065 · búsqueda y lectura integradas en el chat Rust

Estado: implementado y probado con fixtures. No prueba de Internet
o modelos reales ni autorización de compilación/publicación Windows/Linux.

## Decisión y alternativas

Se conecta el lector Rust existente al chat del catálogo local (ADR0064), sin
portar/modificar HTML/CSS/JS. El frontend ya soporta web_reader, runs/status/cancel,
reader_run y fuentes/lecturas en data.web. Se reutiliza exactamente ese protocolo.
No ejecutar Python/curl, copiar otro motor o añadir una UI paralela. El perfil
MLX pinned anterior conserva su presupuesto3584bytes y permanece sin esta nueva
activación; no relajar ese contrato para esconder un contexto insuficiente.

Catálogo v1 admite web_search boolean opcional, false por defecto. Activarlo
requiere cerebro con tools declaradas y presupuesto de entrada≥8192bytes; se
explican proveedor/transferencia de consultas en la revisión antes de iniciar.
Abrir, detectar, guardar, health o crear run no busca. Solo una propuesta de
web_search en un turno autorizado puede contactar al proveedor fijo ya utilizado
por el prototipo: HTTPS html.duckduckgo.com. Sin proveedor alternativo/fallback.

## Seguridad y límites

Transporte público del Reader existente: HTTPS443, DNS validado/IP fijada, TLS,
sin proxy, credenciales, cookies, sesiones de navegador, JS o retries. Búsqueda
sin redirects; lectura conserva≤2 saltos revalidados. Parser HTML acotado propio
con html5ever ya existente; solo títulos/snippets/URLs HTTPS permitidas,≤5 resultados.
3 búsquedas,≤3 páginas únicas en un batch,45s de deadline compartido de red (el
tiempo transcurrido de inferencia también reduce el plazo),≤6 inferencias por turno.
Una vez iniciada lectura ya no se permiten búsquedas: el texto leído no puede
provocar otra consulta/exportación de datos. Nunca URLs libres suministradas por
el modelo; solo IDs del registro de resultados del run actual.

El modelo decide si necesita búsqueda, no un detector de palabras clave; no
forzar Internet en saludos. Fuentes y resultados son mensajes tool no confiables,
nunca mensajes de sistema o permisos. El opt-in explica que se envían consultas
al proveedor; no envía automáticamente la conversación completa a ese proveedor.
No hay Mail/archivos/shell ni herramientas OpenCode añadidas o duplicadas.

Síntesis mediante web_answer estructurado, sin URLs libres; citas únicas solo a
IDs cuyo extracto llegó al modelo. Enlaces los resuelve el host. Texto de fuentes
se recorta con flag explícito, dentro de8000/16000 caracteres y del presupuesto
conservador del modelo; historial/schemas nunca se truncan silenciosamente.
Cada request completo se admite con estimación por bytes/overhead/salida/margen,
sin afirmar tokenizador o ajuste universal al KV/contexto real. Error explícito
context_exhausted cuando no cabe. Validar citas no acredita soporte semántico ni
utilidad; publicación/fechas y calidad real siguen sin evaluar.

## Turnos, estado y cierre

Run/token opacos por host, registro≤128 con TTL10min para inactivos, consumo de
un uso y estado observado. Slot de conversación durante toda la orquestación;
no intercalar otro chat/modelo mientras busca/lee/sintetiza. Herramientas de jobs
solo en la primera decisión antes de búsquedas; no mezclar acciones privilegiadas
o de trabajos después de recibir fuentes. Prosa jamás dispara herramientas.

Auth/Host/Origin/límites preceden a runs/status/cancel. Cancelar antes de upload
impide comenzar; durante inferencia se espera su resultado/deadline sin fingir
liberación. Durante red, cancelación/timeout pone el Reader en cuarentena si hay
I/O incierto. Intención de red persistida antes de DNS; marker retenido si cleanup
no confirmado. No crear otra instancia para eludirlo; sin recuperación automática.
Desconectar TCP o refrescar no reenvía el run. El propietario del turno conserva
la tarea hasta terminar; stop del host revoca admisión/cancela y drena.

## Migración y entrega

Catálogos anteriores siguen sin red por defecto. JSON antiguo, perfil MLX y
API /v1 conservan comportamiento; la búsqueda se limita al camino web habilitado.
Mismo origen/namespace/historial, fuentes escapadas por el frontend existente.
Pruebas de búsqueda HTML/transporte, SSRF, referencias/IDs, citas, errores/parcial,
presupuesto y cancelación con runtimes/páginas sintéticos. Internet/GUI/calidad
de modelos y compilación nativa Windows/Linux quedan para ensayos separados.
