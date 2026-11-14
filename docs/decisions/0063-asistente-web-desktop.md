# ADR0063 · arranque web antes de configurar motores

Estado: implementación en curso. Sustituye el onboarding de ADR0061, no el
contrato desktop v1 ni las puertas del runtime.

## Decisión y alternativas

Los diálogos de arranque obligan a conocer ejecutables y perfiles internos.
Un framework desktop nuevo duplicaría UI/dependencias. Se conserva HTML/CSS del
chat y se añade un asistente web común servido por Rust antes de leer perfiles
o comprobar modelos. Los selectores son adaptadores nativos invocados desde la
web autenticada; no una capacidad para ejecutar comandos recibidos del navegador.

El shell posee 127.0.0.1:8015 durante toda la sesión. El host macOS publica su
router al shell en vez de intentar ocupar el mismo puerto. Ambos usan bootstrap
de un uso y cookies HttpOnly/SameSite con prefijos distintos. No proxy remoto,
puerto del runtime expuesto, CORS permisivo ni secreto en JS/localStorage.
Host/Origin, autenticación, límites de cuerpo y exclusión preceden a selectores,
lecturas de configuración y escrituras. Caducidad/cierre esperan cleanup propio.

Se reutiliza tower (antes dependencia de tests) para despachar el router montado;
sha2, ya fijado transitivamente, calcula revisiones CAS compactas sin exponer el
perfil crudo al navegador. Sin frameworks o paquetes nuevos descargados. Cuerpos
autenticados limitados a128KiB/5s y cuatro lecturas concurrentes; una mutación
principal, con parada independiente para cancelar un inicio pendiente.

## Flujo y alcance

Web sin modelos → detectar rutas conocidas/perfil local → editar/seleccionar →
revisar filesystem y efectos → confirmar guardar/iniciar → chat. Revisión genera
una aprobación opaca de bytes inmutables, consumida una vez; no se relee un JSON
arbitrario al arrancar. Guardado privado atómico con comparación de baseline.
Guardar no equivale a inferencia; iniciar conserva pins/preflight/lease/cuarentena.
Parada visible en la web; Ajustes permite volver al asistente. Sin reenvíos POST
automáticos tras recarga/error de red, sin descargas o cambios de permisos externos.

La UI/shell funciona en Windows/macOS/Linux. Solo macOS dispone del adaptador de
inferencia actual; el shell de los otros sistemas informa la ausencia y bloquea
el arranque. No se implementa aquí el catálogo intercambiable de roles, downloader,
backend Windows ni actualizador. El gestor permite guardar/sustituir IDs y pools
(hasta16 únicos), pero activa solo las asignaciones fijadas actuales. macOS guarda
desktop-manager.json privado/atómico y conserva desktop.json como rollback.
Borrador explícito de solo IDs en localStorage también disponible en Windows;
no equivale a un perfil de inferencia o instalación de modelos.

## Migración, pruebas y rollback

desktop.json existente se ofrece como configuración, nunca arranca por abrir.
Se conservan origen 8015, almacenamiento del navegador y data/coordination.
No se reparan permisos de carpetas existentes automáticamente: el error se explica
en la web. No se borran perfiles, modelos o historial. --serve y Python siguen
siendo rollback. Pruebas HTTP/DOM con adaptadores sintéticos, build e inspección
de paquete sin instalar; GUI/Finder e inferencia real requieren ensayo separado.
