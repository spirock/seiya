# Seiya · hoja de ruta pública

Seiya se desarrolla de forma incremental. Los paquetes son experimentales: un
build correcto no acredita instalación, inferencia o paridad entre plataformas.
Descargas verificadas y matriz por sistema: [README](README.md).

## Estado de entrega

- Ejecutables Rust y paquetes separados del prototipo Python para macOS ARM64,
  Linux x64 y Windows x64. Sin pesos, motores de inferencia o credenciales incluidos.
- CI compila, prueba contratos e inspecciona los payloads sin instalar paquetes.
  Acciones y pruebas de interfaz usan Node24.
- Shell web común y catálogo HTTP local experimental en las tres plataformas;
  Windows/Linux no tienen la inferencia/imagen nativa macOS validada.
- Historial de conversaciones por origen del navegador y snapshots Rust para
  trabajos/PNG, sin reenvío automático al recuperar.

## En validación · experiencia macOS

Fuentes actuales: shell web-first común sin modelos/perfil y catálogo de texto
con cerebro/trabajadores sustituibles, guardado CAS y backend HTTP local externo
compartido Windows/Linux/macOS. Pool secuencial y controles de binding, recursos,
exclusión e incertidumbre; los motores HTTP se preparan externamente.
Imagen/chat opt-in, búsqueda web y detección Ollama con rutas editables. macOS
incorpora lifecycle nativo secuencial y journal durable, conservando pins y límite
3584bytes. No mide VRAM liberada ni añade lifecycle genérico a APIs externas.
Frontend HTML/CSS/JS permanente
y assets congelados; sin portarlo o reemplazar el diseño.
Identificar el build descargado por SHA/run; pruebas sintéticas, no GUI real.
[Operación del catálogo y límites](docs/BACKENDS_LOCALES.md).

Build anterior: launcher con configuración/elegido de perfil, revisión de rutas, confirmación de
arranque, web autenticada solo loopback y control de parada. El diseño del chat se
conserva. Las pruebas de diálogos/browser/host usan adaptadores simulados; Finder,
sesiones largas, modelos y presupuesto de memoria requieren validación real.
[Operación](docs/APP_RUST.md) · [Matriz de paridad](docs/PARIDAD_RUST.md).

## Planificado · modelos por roles

Un modelo central, un pipeline de imágenes y una colección de trabajadores para
tareas pesadas. Elecciones sustituibles por motores/modelos compatibles, no marcas
obligatorias. Importación, procedencia/licencias, capacidades, límites y rollback
antes de activar nuevas asignaciones. Instalado/habilitado no significa residente;
perfil conservador secuencial y paralelismo solo con mediciones que lo respalden.
[Diseño de roles](docs/decisions/0062-modelos-por-roles.md).

## Planificado · versiones y actualizaciones

Versión de producto y build identificables, manifiesto público con última versión
y descargas por plataforma. Aviso de actualización al abrir y como máximo diario
mientras la app esté abierta, según permisos de red. Sin instalar/ejecutar paquetes
automáticamente, enviar conversaciones o bloquear uso offline. No requiere daemon
cuando la app está cerrada. La comparación debe detectar versiones posteriores,
no proponer downgrades por mera desigualdad. Este mecanismo aún no está implementado.

## Planificado · ampliar compatibilidad

Backends de inferencia y modelos validados por plataforma, paridad de funciones
pendientes y recuperación/actualización sin pérdida de datos. Firma/notarización,
instalación y rollback en dispositivos reales antes de promocionar una release
estable. No retirar el prototipo operativo hasta superar esas puertas.

## Criterios de avance

Separar siempre especificación, implementación, pruebas simuladas y validación
real. Publicar capacidades y límites del build descargable, sin promesas de RAM,
rendimiento o compatibilidad de todos los modelos por compilar un ejecutable.
Los planes de trabajo y las instrucciones de desarrollo internas no forman parte
del repositorio público. Esta hoja contiene únicamente hitos y estado del producto.
