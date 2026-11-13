# ADR0062 · cerebro, imágenes y grupo de trabajadores sustituibles

Estado: requisito de producto; diseño técnico pendiente.
No implementado ni integrado todavía en el launcher. No modifica interfaces v1,
tags/pins del ensayo o permisos de descarga/carga/ejecución.

## Decisión de producto

Seiya/Pegassus no se liga permanentemente a Gemma, Qwen o DeepSeek. El usuario
configura roles y motores compatibles con su dispositivo:

1. **Central/cerebro:** un modelo seleccionado por perfil para conversar, entender
   la petición, planificar, asignar trabajos y sintetizar resultados. Coordinación
   de recursos/permisos sigue en Rust; no se delega la autoridad al LLM.
2. **Diseño/generación de imágenes:** un pipeline seleccionado, con sus componentes
   necesarios (difusión, encoder, VAE según motor), bajo solicitud explícita. No
   prometer que cambiar un único archivo cambia todo el pipeline o habilita visión.
3. **Trabajadores fuertes:** una colección configurable de modelos para tareas
   pesadas. Qwen/DeepSeek son elecciones sustituibles, no nombres obligatorios ni
   evidencia de que todo modelo de esas familias esté instalado/soportado.
   Selección por especialidad/capacidades antes de iniciar cada tarea, entre los
   modelos habilitados y admitidos por la política del usuario.

Alternativas rechazadas: fijar un modelo por marca para todos los equipos, asumir
que todos los trabajadores deben residir simultáneamente o convertir cualquier
archivo renombrado en un backend compatible.

## Recursos y programación

Separar **registrado/instalado**, **habilitado para un rol**, **validado para ese
motor/dispositivo**, **residente** y **ejecutando**. Tener cinco trabajadores
configurados no implica cinco procesos o pesos cargados. La UI debe mostrar cada
estado sin confundir cola con ejecución o filesystem con validación real.

La configuración/recomendación depende de plataforma, CPU/GPU, RAM/VRAM (o memoria
unificada), memoria libre/comprometida, presión, contexto/KV y runtime. El tamaño
de los pesos no es una medida suficiente de RAM. Presupuestos por dispositivo
se miden; no fijar números de workers seguros solo por tener16/32/64GB.

Default conservador: turnos exclusivos, con descarga/cierre confirmado antes del
siguiente modelo. El cerebro puede descargarse mientras trabaja un fortachón;
Rust y la UI siguen disponibles, pero no prometer chat LLM simultáneo. Paralelismo
solo tras adaptadores/mediciones y política autorizada que lo acrediten. Unknown/
critical no conceden admisión. Respetar MEMORIA_ADAPTATIVA, leases y cuarentena.

## Gestión de modelos y límites

Importar o registrar modelos existentes y, más adelante, descargas confirmadas
con licencia/procedencia/hashes. Validar formato real, plantilla/tokenizer,
capacidades de tools/modalidad, motor y SO; perfiles MLX no se vuelven Windows por
copiar pesos. Cambiar asignación se aplica a trabajos nuevos, no altera un run
activo, no trunca contexto ni cambia a remoto/otro modelo para ocultar errores.

Resultados/propuestas de cualquier worker son datos no confiables. El rol de
trabajador no concede shell, escritura de proyectos, acceso a archivos o ejecución
de tests: cada capacidad necesitará su contrato, grants y executor aislado. Hoy
el auxiliar del ensayo solo produce borradores; este requisito no amplía ese permiso.

Guardar versiones/asignaciones y rollback sin borrar pesos, historial o perfil
anterior. La futura UI Modelos debe permitir elegir cerebro, pipeline de imágenes
y añadir/quitar/habilitar trabajadores, con límites visibles y sin autoload.
La configuración actual de Gemma/Qwen es un perfil inicial de compatibilidad,
no el diseño final del catálogo. La matriz de soporte tendrá evidencia por motor,
rol y plataforma; modelos no soportados se muestran como tales.
