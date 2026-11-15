# Chat de texto · recuperación y límites

El chat web conserva su diseño HTML/CSS/JavaScript e historial en el navegador.
El complemento PreparedWeb valida la respuesta antes de guardarla o mostrarla:
una respuesta incompleta o un estado de fuentes/trabajos inválido produce error,
no una respuesta fantasma. Si falta tiempo medido muestra «— s».

## Mensajes sin respuesta después de recargar

«Recuperar mensaje sin respuesta» mueve el último mensaje del usuario al
compositor. No lo envía ni reemplaza un borrador distinto. Si no se puede guardar
el cambio, conserva mensaje y borrador originales. Después puedes editar y
enviar manualmente, sin añadir dos veces el mismo turno al historial.

Un envío perdido puede haber llegado al modelo: **comprueba el runtime antes de
reenviar**. El botón no consulta ni confirma su parada. Revisar conexión tampoco
demuestra que una inferencia anterior haya terminado. No hay retries automáticos.

## Errores y escritura

Los rechazos401/403 pausan las consultas de recuperación incluso cuando el
servidor no devuelve JSON. Abre el acceso vigente de Seiya y revisa conexión;
no publiques claves, cookies ni URL de sesión. La sesión local no tiene un timer,
pero cerrar/reiniciar Seiya revoca el acceso anterior.

Durante una respuesta puedes escribir otro borrador: el resultado no lo borra.
Enter envía, Shift+Enter añade línea y la composición IME no envía por accidente.
La respuesta sigue siendo bufferizada, no streaming de tokens. Abortar una espera
HTTP no acredita cancelación del runtime ni liberación de RAM/VRAM.

## Configuración dentro del chat

Las fuentes del incremento de submenú añaden dos opciones al pulsar
**Configuración**: **Configuración de modelos** y **Configuración general**.
Se muestran en el área central sin abandonar la conversación. El acceso
«Configurar modelos» de la cabecera abre la misma vista.

Volver al chat, Escape o seleccionar otra conversación cierra la vista sin borrar
historial, borrador o campos editados. Durante un envío no se cambia de vista.
Abrir/alternar ajustes no guarda ni inicia motores. Modelos reutiliza el asistente
y sus APIs autenticadas, con detección/estado solo al primer acceso; general
conserva los controles actuales de opciones, límites y diagnóstico.

Guardar, iniciar o detener siguen requiriendo sus acciones explícitas y las
guardas existentes. Si se cambia de host, «Abrir chat» sigue abriendo el acceso
nuevo correspondiente; ocultar los ajustes no recupera un runtime detenido.
No se usan iframes ni se relajan las protecciones del navegador. Un host sin
gestor de modelos o diagnóstico no anuncia esos controles como disponibles.
Este cambio de fuentes no está en el instalador b8ae9e2 publicado anteriormente.
Validación del submenú:502 pruebas Rust y196 Python,9 suites Node correctas en
serie;25 pruebas ignoradas. Formato/Clippy correctos, seis assets congelados sin
cambios. No acredita interacción real en Safari ni recupera un runtime bloqueado.

## Comprobación y rollback

`node swarm-app/tests/chat_text_ui.cjs` usa DOM/HTTP sintéticos. Cubre éxito,
respuesta inválida, metadatos de tiempo ausentes, error auth sin JSON, borrador
concurrente, doble envío, recuperación/reload sin replay, fallo localStorage,
teclado e interacción con la referencia idempotente de chat/imagen.
La prueba HTTP Rust verifica que el host inyecta el complemento.

Regresión de este incremento:499 pruebas Rust y196 Python,7 suites Node correctas
en ejecución serial,24 pruebas ignoradas; formato/Clippy correctos. La ejecución
concurrente inicial falló en una fixture nativa de limpieza sin cambios
(IdentityChanged), que pasó aislada y en serie; su causa no se da por corregida.

No acredita instalación, Safari/otros navegadores reales, inferencia sostenida o
calidad de texto/imágenes. No se ha modificado OpenCode ni cargado modelos.
Para rollback de este incremento, retirar la inyección chat_text_ui y el ajuste
de parsing HTTP en PreparedWeb; no borrar historial/cookies/configuraciones.
No hay migración ni nuevo formato de datos persistidos.
