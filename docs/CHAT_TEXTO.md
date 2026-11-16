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

### Probar sin instalar

Para probar el **chat real** en lugar de la preview limitada, compilar en modo
debug y ejecutar `seiya-desktop --test-chat`: no se imprime enlace secreto.
En `--test-chat` de desarrollo, entrar directamente en
`http://localhost:8016/app/` crea automáticamente `seiya_test_access`
(HttpOnly/SameSite Strict, Path=/app/) y abre el shell completo; se puede volver
a usar la misma dirección en un navegador nuevo o tras reiniciar. El navegador
se abre también directamente en esa ruta, sin enlace bootstrap manual.
**Solo en este modo debug**, cualquier programa local puede obtener una cookie
visitando `/app/`: no es autenticación de usuario. La build release rechaza
`--test-chat` y no activa esta entrada pública de desarrollo. La llave local
privada del shell de pruebas
persiste entre reinicios, sin revocación por reiniciar. Safari puede borrar o
limitar cookies; abrir de nuevo la aplicación restablece la cookie. Nunca se
imprime el bootstrap en consola. `8015` conserva su acceso de un solo uso.
El chat montado al iniciar el host sigue teniendo su propia autenticación y
guardas; iniciarlo exige acción explícita y no se garantiza inferencia disponible.
No instala paquetes ni desplaza la app de8015. Usa el adaptador macOS y perfil
reales, así que **revisar/guardar cambia configuración privada real** e iniciar
host o enviar mensaje puede cargar Gemma; el usuario debe hacerlo explícitamente.
Comparte locks y marcadores con la app instalada: errores de exclusión o cierre
incierto permanecen bloqueantes, sin limpieza/reintento automático. Antes de
preparar host el shell no permite enviar, como en producción; después de
confirmar e iniciar, «Abrir chat» lleva al chat normal con compositor habilitado.
No confundir abrir enlace con inferencia probada: no se garantiza que el runtime
acepte Gemma en el estado actual. No se genera una respuesta simulada.

La opción `--preview` anterior sigue siendo explícitamente solo UI y memoria:

`seiya-desktop --preview` abre un servidor de prueba solo en127.0.0.1:8016 y
imprime su enlace autenticado de un solo uso. Para pruebas usar siempre8016;
8015 queda reservado a la app instalada, que no se cierra ni se adopta. Si8016
está ocupado, falla sin cambiar de puerto ni terminar procesos ajenos.
Es **vista previa de interfaz**, no inferencia: permite revisar/guardar campos
solo en memoria y rechaza iniciar modelos y selectores de archivos. No lee
perfiles, historiales, documentos o marcadores de la instalación. La conversación
real no está habilitada ni se presenta una respuesta sintética como generada.
El banner identifica esta limitación. Cerrar/reiniciar preview revoca su acceso y
descarta su configuración temporal; no cambia sesiones o cookies de8015.

Corrección del shell inicial (posterior al build610d697): también antes de preparar
el host, Configuración abre el submenú modelos/general y ocupa exclusivamente el
área central. No hay panel flotante ni conversación/compositor detrás de los
ajustes. Volver/Escape recuperan la superficie inicial sin activar inferencia.
Guardar sin iniciar, tras confirmación exitosa, cierra la vista y muestra el
resultado; un error no cierra ni reintenta. Los campos editados se conservan.
En el shell inicial General informa que el diagnóstico requiere host preparado;
no anuncia controles inexistentes. El chat sigue inerte hasta preparar el host.
Esta corrección no está incluida en el instalador610d697 y no elimina cuarentena.
Comprobaciones de esta corrección:503 Rust+196 Python y10 suites Node correctas,
25 ignored; fmt/Clippy/diff-check y6 hashes frozen intactos. DOM/HTTP sintéticos,
sin validación de Safari/instalación real o inferencia. Build release desktop
compilado e inspeccionado sin ejecutar.

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
