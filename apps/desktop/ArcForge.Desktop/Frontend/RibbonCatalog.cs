namespace ArcForge.Desktop.Frontend;

public sealed record RibbonCommandDefinition(string ActionId, string Label, string Icon)
{
    private RibbonCommandSemantics Semantics => RibbonCatalog.GetSemantics(ActionId);

    public UiActionKind Kind => Semantics.Kind;

    public string Intent => Semantics.Intent;

    public bool IsBackendDependent => Semantics.IsBackendDependent;

    public bool IsTool => Semantics.IsTool;

    public bool HasVariants =>
        // Commands that open a menu (Kind=Flyout) must advertise the same '⌄' affordance as
        // the tools with explicit variants: annotation scale, new/two windows, and so on.
        Kind == UiActionKind.Flyout ||
        ActionId is
        "ribbon.anotar.texto" or
        "ribbon.anotar.cota" or
        "ribbon.anotar.directriz" or
        "ribbon.anotar.multidirectriz" or
        "ribbon.vistas.zoom" or
        "ribbon.vistas.encuadre" or
        "ribbon.vistas.delimitar" or
        "ribbon.herramientas.distancia" or
        "ribbon.herramientas.area" or
        "ribbon.herramientas.seleccionar";

    public string VariantActionId => $"{ActionId}.variants";

    public string VariantAutomationName => $"Variantes de {Label}";

    public string? Shortcut => Semantics.Shortcut;

    public string Tooltip
    {
        get
        {
            var shortcut = string.IsNullOrWhiteSpace(Shortcut) ? string.Empty : $"\nAtajo: {Shortcut}";
            var availability = ActionId is
                "ribbon.herramientas.distancia" or
                "ribbon.herramientas.angulo" or
                "ribbon.herramientas.id-de-punto" or
                "ribbon.herramientas.lista"
                    ? string.Empty
                    : "\nNo disponible: esta capacidad aún no está conectada";
            return $"{Label}\n{Intent}{availability}{shortcut}";
        }
    }
}

internal readonly record struct RibbonCommandSemantics(
    UiActionKind Kind,
    string Intent,
    bool IsBackendDependent = false,
    string? Shortcut = null,
    bool IsTool = false);

public sealed record RibbonGroupDefinition(
    string Label,
    double Width,
    RibbonCommandDefinition[] Commands)
{
    public bool HasLauncher => Label is "Texto" or "Cotas" or "Bloques" or "Selección" or "Capas" or "Trazar" or "PDF y DWF";

    public bool IsLabelOnly => !HasLauncher;

    // The last group of a contextual tab must not paint a trailing right separator; otherwise it
    // floats as a loose vertical line in the empty band after the ribbon content.
    public bool IsLast { get; init; }
}

public static class RibbonCatalog
{
    public static IReadOnlyDictionary<string, RibbonGroupDefinition[]> Sections { get; } = BuildSections();

    private static Dictionary<string, RibbonGroupDefinition[]> BuildSections()
    {
        var sections = new Dictionary<string, RibbonGroupDefinition[]>(StringComparer.Ordinal)
        {
            ["Anotar"] =
            [
                G("Texto", 280,
                    C("ribbon.anotar.texto", "Texto", "Text"),
                    C("ribbon.anotar.multilinea", "Multilínea", "Multiline"),
                    C("ribbon.anotar.directriz", "Directriz", "Leader"),
                    C("ribbon.anotar.campo", "Campo", "Attribute")),
                G("Cotas", 360,
                    C("ribbon.anotar.cota", "Cota", "Dimension"),
                    C("ribbon.anotar.lineal", "Lineal", "DimLinear"),
                    C("ribbon.anotar.angular", "Angular", "Angular"),
                    C("ribbon.anotar.radio", "Radio", "Radius"),
                    C("ribbon.anotar.estilo", "Estilo", "Properties")),
                G("Directrices", 260,
                    // A two-word standard CAD term wraps cleanly across two tile
                    // lines instead of splitting the single long label in the middle of a word.
                    C("ribbon.anotar.multidirectriz", "Directriz múltiple", "Multileader"),
                    C("ribbon.anotar.alinear", "Alinear", "Move"),
                    C("ribbon.anotar.marca-de-centro", "Marca de centro", "CenterMark")),
                G("Tablas y marcas", 300,
                    C("ribbon.anotar.tabla", "Tabla", "Table"),
                    C("ribbon.anotar.vinculo", "Vínculo", "Insert"),
                    C("ribbon.anotar.nube-de-revision", "Nube de revisión", "RevCloud"),
                    C("ribbon.anotar.escala-anotativa", "Escala anotativa", "AnnoScale")),
            ],
            ["Insertar"] =
            [
                G("Bloques", 320,
                    C("ribbon.insertar.insertar", "Insertar", "Insert"),
                    C("ribbon.insertar.crear-bloque", "Crear bloque", "Block"),
                    C("ribbon.insertar.atributos", "Atributos", "Attribute"),
                    C("ribbon.insertar.punto-base", "Punto base", "ChangeBase")),
                G("Referencias", 360,
                    C("ribbon.insertar.asociar-dwg", "Asociar DWG", "AttachDwg"),
                    C("ribbon.insertar.imagen", "Imagen", "Image"),
                    C("ribbon.insertar.pdf", "PDF", "Pdf"),
                    C("ribbon.insertar.xref", "Xref", "Xref"),
                    C("ribbon.insertar.recargar", "Recargar", "Sync")),
                G("Importar y datos", 260,
                    C("ribbon.insertar.importar-pdf", "Importar PDF", "PdfImport"),
                    C("ribbon.insertar.extraer-datos", "Extraer datos", "ExtractData"),
                    C("ribbon.insertar.vinculo-de-datos", "Vínculo de datos", "Table")),
                G("Contenido", 260,
                    C("ribbon.insertar.paleta-de-bloques", "Paleta de bloques", "Palettes"),
                    C("ribbon.insertar.objeto-ole", "Objeto OLE", "Paste"),
                    C("ribbon.insertar.ubicacion", "Ubicación", "Location")),
            ],
            ["Vistas"] =
            [
                G("Vistas modelo", 300,
                    C("ribbon.vistas.superior", "Superior", "ViewTop"),
                    C("ribbon.vistas.frontal", "Frontal", "ViewFront"),
                    C("ribbon.vistas.isometrica", "Isométrica", "ViewIso"),
                    C("ribbon.vistas.guardar-vista", "Guardar vista", "Save")),
                G("Navegación", 280,
                    C("ribbon.vistas.zoom", "Zoom", "Zoom"),
                    C("ribbon.vistas.encuadre", "Encuadre", "Pan"),
                    C("ribbon.vistas.orbita", "Órbita", "Orbit"),
                    C("ribbon.vistas.extension", "Extensión", "ZoomExtents")),
                G("Ventanas gráficas", 320,
                    C("ribbon.vistas.nueva-ventana", "Nueva ventana", "Paper"),
                    C("ribbon.vistas.dos-ventanas", "Dos ventanas", "Copy"),
                    C("ribbon.vistas.bloquear", "Bloquear", "ViewLock"),
                    C("ribbon.vistas.delimitar", "Delimitar", "Boundary")),
                G("Estilos visuales", 300,
                    C("ribbon.vistas.alambrica", "Alámbrica", "Linetype"),
                    C("ribbon.vistas.sombreado", "Sombreado", "Hatch"),
                    C("ribbon.vistas.realista", "Realista", "Realistic"),
                    C("ribbon.vistas.pantalla-completa", "Pantalla completa", "Fullscreen")),
            ],
            ["Herramientas"] =
            [
                G("Medir", 320,
                    C("ribbon.herramientas.distancia", "Distancia", "Measure"),
                    C("ribbon.herramientas.area", "Área", "Area"),
                    C("ribbon.herramientas.angulo", "Ángulo", "Angular"),
                    C("ribbon.herramientas.volumen", "Volumen", "Boundary")),
                G("Consultar", 280,
                    C("ribbon.herramientas.id-de-punto", "ID de punto", "PointId"),
                    C("ribbon.herramientas.lista", "Lista", "Properties"),
                    C("ribbon.herramientas.propiedades-fisicas", "Prop. físicas", "MassProps"),
                    C("ribbon.herramientas.calculadora", "Calc.", "Calculator")),
                G("Selección", 320,
                    C("ribbon.herramientas.seleccionar", "Seleccionar", "Selection"),
                    C("ribbon.herramientas.seleccion-rapida", "Selección rápida", "Filter"),
                    C("ribbon.herramientas.similar", "Similar", "Similar"),
                    C("ribbon.herramientas.invertir", "Invertir", "Array")),
                G("Utilidades", 280,
                    C("ribbon.herramientas.auditar", "Auditar", "Audit"),
                    C("ribbon.herramientas.limpiar", "Limpiar", "Purge"),
                    C("ribbon.herramientas.recuperar", "Recuperar", "Open"),
                    C("ribbon.herramientas.opciones", "Opciones", "Settings")),
            ],
            ["Smart"] =
            [
                G("Asistente Arc", 280,
                    C("ribbon.smart.asistente", "Asistente", "Assistant"),
                    C("ribbon.smart.sugerir-comando", "Sugerir comando", "Suggest"),
                    C("ribbon.smart.explicar-seleccion", "Explicar selección", "Properties")),
                G("Selección inteligente", 300,
                    C("ribbon.smart.smart-select", "Smart Select", "Selection"),
                    C("ribbon.smart.por-propiedades", "Por propiedades", "Filter"),
                    C("ribbon.smart.similar", "Similar", "Similar"),
                    C("ribbon.smart.agrupar", "Agrupar", "Array")),
                G("Reconocimiento", 320,
                    C("ribbon.smart.detectar-geometria", "Detectar geometría", "Boundary"),
                    C("ribbon.smart.muros", "Muros", "Rectangle"),
                    C("ribbon.smart.aberturas", "Aberturas", "Opening"),
                    C("ribbon.smart.simbolos", "Símbolos", "Block")),
                G("Optimizar", 300,
                    C("ribbon.smart.limpiar-dibujo", "Limpiar dibujo", "CleanDrawing"),
                    C("ribbon.smart.unir", "Unir", "Polyline"),
                    C("ribbon.smart.duplicados", "Duplicados", "Duplicates"),
                    C("ribbon.smart.simplificar", "Simplificar", "Stretch")),
            ],
            ["Administra"] =
            [
                G("Capas", 320,
                    C("ribbon.administra.propiedades", "Propiedades", "LayerProperties"),
                    C("ribbon.administra.estados", "Estados", "LayerStates"),
                    C("ribbon.administra.aislar", "Aislar", "Isolate"),
                    C("ribbon.administra.combinar", "Combinar", "Merge")),
                G("Estilos", 300,
                    C("ribbon.administra.texto", "Texto", "Text"),
                    C("ribbon.administra.cota", "Cota", "Dimension"),
                    C("ribbon.administra.tabla", "Tabla", "Table"),
                    C("ribbon.administra.directriz", "Directriz", "Leader")),
                G("Estándares", 280,
                    C("ribbon.administra.comprobar", "Comprobar", "CheckStandards"),
                    C("ribbon.administra.configurar", "Configurar", "Sliders"),
                    C("ribbon.administra.traducir-capas", "Traducir capas", "LayerTranslate"),
                    C("ribbon.administra.actualizar", "Actualizar", "Sync")),
                G("Mantenimiento", 300,
                    C("ribbon.administra.auditar", "Auditar", "Audit"),
                    C("ribbon.administra.limpiar", "Limpiar", "Purge"),
                    C("ribbon.administra.recuperar", "Recuperar", "Open"),
                    C("ribbon.administra.personalizar-iu", "Personalizar IU", "Palettes")),
            ],
            ["Exportar"] =
            [
                G("Trazar", 300,
                    C("ribbon.exportar.trazar", "Trazar", "Print"),
                    C("ribbon.exportar.configurar-pagina", "Configurar página", "Paper"),
                    C("ribbon.exportar.vista-previa", "Vista previa", "View"),
                    C("ribbon.exportar.publicar", "Publicar", "Publish")),
                G("PDF y DWF", 260,
                    C("ribbon.exportar.exportar-pdf", "Exportar PDF", "Pdf"),
                    C("ribbon.exportar.dwf", "DWF", "FormatDwf"),
                    C("ribbon.exportar.opciones", "Opciones", "Settings")),
                G("Intercambio CAD", 340,
                    C("ribbon.exportar.dwg", "DWG", "FormatDwg"),
                    C("ribbon.exportar.dxf", "DXF", "FormatDxf"),
                    C("ribbon.exportar.guardar-copia", "Guardar copia", "SaveAll"),
                    C("ribbon.exportar.version", "Versión", "Version")),
                G("Imagen y 3D", 300,
                    C("ribbon.exportar.png-jpg", "PNG / JPG", "Image"),
                    C("ribbon.exportar.svg", "SVG", "Polyline"),
                    C("ribbon.exportar.stl-obj", "STL / OBJ", "Mesh3d"),
                    C("ribbon.exportar.compartir", "Compartir", "ShareLink")),
            ],
            ["En línea"] =
            [
                G("ArcCloud", 300,
                    C("ribbon.en-linea.abrir-nube", "Abrir nube", "Cloud"),
                    C("ribbon.en-linea.guardar", "Guardar", "Save"),
                    C("ribbon.en-linea.sincronizar", "Sincronizar", "CloudSync"),
                    C("ribbon.en-linea.sin-conexion", "Sin conexión", "CloudOff")),
                G("Compartir", 300,
                    C("ribbon.en-linea.compartir-vinculo", "Compartir vínculo", "ShareLink"),
                    C("ribbon.en-linea.comentarios", "Comentarios", "Comment"),
                    C("ribbon.en-linea.permisos", "Permisos", "Permissions"),
                    C("ribbon.en-linea.avisos", "Avisos", "Bell")),
                G("Versiones", 300,
                    C("ribbon.en-linea.historial", "Historial", "History"),
                    C("ribbon.en-linea.comparar", "Comparar", "Compare"),
                    C("ribbon.en-linea.restaurar", "Restaurar", "Restore"),
                    C("ribbon.en-linea.crear-version", "Crear versión", "Version")),
                G("Cuenta y recursos", 300,
                    C("ribbon.en-linea.actividad", "Actividad", "Properties"),
                    C("ribbon.en-linea.cuenta", "Cuenta", "Account"),
                    C("ribbon.en-linea.ayuda", "Ayuda", "Help"),
                    C("ribbon.en-linea.plantillas", "Plantillas", "New")),
            ],
            ["Geoservicio"] =
            [
                G("Ubicación", 280,
                    C("ribbon.geoservicio.definir-ubicacion", "Definir ubicación", "Location"),
                    C("ribbon.geoservicio.buscar-direccion", "Buscar dirección", "Search"),
                    C("ribbon.geoservicio.marcar-punto", "Marcar punto", "MarkPoint")),
                G("Coordenadas", 280,
                    C("ribbon.geoservicio.asignar-src", "Asignar SRC", "Ducs"),
                    C("ribbon.geoservicio.transformar", "Transformar", "Rotate"),
                    C("ribbon.geoservicio.unidades", "Unidades", "Units")),
                G("Mapas", 320,
                    C("ribbon.geoservicio.mapa-base", "Mapa base", "MapBase"),
                    C("ribbon.geoservicio.satelite", "Satélite", "Satellite"),
                    C("ribbon.geoservicio.terreno", "Terreno", "Hatch"),
                    C("ribbon.geoservicio.en-linea", "En línea", "CloudSync")),
                G("Datos GIS", 320,
                    C("ribbon.geoservicio.capas-gis", "Capas GIS", "LayerProperties"),
                    C("ribbon.geoservicio.importar-shp", "Importar SHP", "Open"),
                    C("ribbon.geoservicio.actualizar", "Actualizar", "Sync"),
                    C("ribbon.geoservicio.exportar-gis", "Exportar GIS", "ExportGis")),
            ],
        };

        // Mark the trailing group of every contextual tab so its right separator is suppressed
        // Commands are preserved, so the 139-command invariant is unchanged.
        foreach (var groups in sections.Values)
        {
            if (groups.Length > 0)
            {
                groups[^1] = groups[^1] with { IsLast = true };
            }
        }

        return sections;
    }

    public static IReadOnlyList<RibbonCommandDefinition> AllCommands { get; } =
        Sections.Values.SelectMany(groups => groups).SelectMany(group => group.Commands).ToArray();

    internal static RibbonCommandSemantics GetSemantics(string actionId) => actionId switch
    {
        // Annotate
        "ribbon.anotar.texto" => Tool("Inicia texto multilínea y solicita su ubicación.", "MT"),
        "ribbon.anotar.multilinea" => Tool("Inicia una anotación de varias líneas."),
        "ribbon.anotar.directriz" => Tool("Inicia una directriz con texto asociado.", "LEADER"),
        "ribbon.anotar.campo" => Modal("Configura un campo vinculado a datos del dibujo.", true),
        "ribbon.anotar.cota" => Tool("Inicia la acotación inteligente.", "DIM"),
        "ribbon.anotar.lineal" => Tool("Inicia una cota lineal.", "DLI"),
        "ribbon.anotar.angular" => Tool("Inicia una cota angular.", "DAN"),
        "ribbon.anotar.radio" => Tool("Inicia una cota de radio.", "DRA"),
        "ribbon.anotar.estilo" => Modal("Abre el editor local de estilos de cota.", true, "D"),
        "ribbon.anotar.multidirectriz" => Tool("Inicia una directriz múltiple.", "MLEADER"),
        "ribbon.anotar.alinear" => Tool("Alinea directrices seleccionadas."),
        "ribbon.anotar.marca-de-centro" => Tool("Coloca una marca de centro."),
        "ribbon.anotar.tabla" => Tool("Inicia la colocación de una tabla.", "TABLE"),
        "ribbon.anotar.vinculo" => Modal("Configura un vínculo de datos para una tabla.", true),
        "ribbon.anotar.nube-de-revision" => Tool("Dibuja una nube de revisión.", "REVCLOUD"),
        "ribbon.anotar.escala-anotativa" => Flyout("Abre el selector de escala anotativa."),

        // Insert
        "ribbon.insertar.insertar" => Modal("Abre la configuración de inserción de bloques.", true, "I"),
        "ribbon.insertar.crear-bloque" => Modal("Configura una definición de bloque.", true, "B"),
        "ribbon.insertar.atributos" => Modal("Configura atributos de bloque.", true, "ATTDEF"),
        "ribbon.insertar.punto-base" => Tool("Solicita un nuevo punto base para el contenido.", "BASE"),
        "ribbon.insertar.asociar-dwg" => Modal("Prepara la asociación de una referencia DWG.", true, "XATTACH"),
        "ribbon.insertar.imagen" => Modal("Prepara la asociación de una imagen.", true, "IMAGEATTACH"),
        "ribbon.insertar.pdf" => Modal("Prepara la asociación de un PDF.", true, "PDFATTACH"),
        "ribbon.insertar.xref" => Dock("Abre la paleta de referencias externas.", true, "XREF"),
        "ribbon.insertar.recargar" => Unavailable("Recarga referencias externas cuando exista acceso a documentos."),
        "ribbon.insertar.importar-pdf" => Modal("Configura la importación de geometría desde PDF.", true, "PDFIMPORT"),
        "ribbon.insertar.extraer-datos" => Modal("Configura una extracción de datos del dibujo.", true, "DATAEXTRACTION"),
        "ribbon.insertar.vinculo-de-datos" => Modal("Administra vínculos de datos externos.", true, "DATALINK"),
        "ribbon.insertar.paleta-de-bloques" => Dock("Abre la biblioteca local simulada de bloques."),
        "ribbon.insertar.objeto-ole" => Modal("Prepara la inserción de un objeto OLE.", true),
        "ribbon.insertar.ubicacion" => Unavailable("Define una ubicación geográfica cuando estén disponibles los geoservicios."),

        // Views
        "ribbon.vistas.superior" => Immediate("Cambia inmediatamente a la vista superior."),
        "ribbon.vistas.frontal" => Immediate("Cambia inmediatamente a la vista frontal."),
        "ribbon.vistas.isometrica" => Immediate("Cambia inmediatamente a la vista isométrica."),
        "ribbon.vistas.guardar-vista" => Modal("Abre el formulario para guardar la vista actual."),
        "ribbon.vistas.zoom" => Tool("Activa la navegación por zoom.", "Z"),
        "ribbon.vistas.encuadre" => Tool("Activa el encuadre interactivo.", "P"),
        "ribbon.vistas.orbita" => Tool("Activa la órbita de la vista.", "3DORBIT"),
        "ribbon.vistas.extension" => Immediate("Ajusta la vista a la extensión del lienzo.", "Z ↵ E"),
        "ribbon.vistas.nueva-ventana" => Flyout("Abre las configuraciones de una nueva ventana gráfica."),
        "ribbon.vistas.dos-ventanas" => Flyout("Abre las disposiciones de dos ventanas gráficas."),
        "ribbon.vistas.bloquear" => Toggle("Alterna el bloqueo de la ventana gráfica."),
        "ribbon.vistas.delimitar" => Tool("Inicia la delimitación de una ventana gráfica."),
        "ribbon.vistas.alambrica" => Immediate("Aplica el estilo visual Alámbrica 2D."),
        "ribbon.vistas.sombreado" => Immediate("Aplica el estilo visual Sombreado."),
        "ribbon.vistas.realista" => Immediate("Aplica el estilo visual Realista."),
        "ribbon.vistas.pantalla-completa" => Immediate("Alterna el modo de pantalla completa.", "Ctrl+0"),

        // Tools
        "ribbon.herramientas.distancia" => Tool("Mide la distancia entre puntos.", "DI"),
        "ribbon.herramientas.area" => Tool("Mide un área definida por puntos u objetos.", "AREA"),
        "ribbon.herramientas.angulo" => Tool("Mide el ángulo entre direcciones."),
        "ribbon.herramientas.volumen" => Tool("Mide el volumen de una selección."),
        "ribbon.herramientas.id-de-punto" => Tool("Consulta las coordenadas de un punto.", "ID"),
        "ribbon.herramientas.lista" => Tool("Solicita un objeto y muestra sus datos.", "LI"),
        "ribbon.herramientas.propiedades-fisicas" => Modal("Muestra un informe de propiedades físicas de la selección.", true, "MASSPROP"),
        "ribbon.herramientas.calculadora" => Modal("Abre la calculadora rápida de la interfaz.", false, "QUICKCALC"),
        "ribbon.herramientas.seleccionar" => Tool("Activa la herramienta de selección."),
        "ribbon.herramientas.seleccion-rapida" => Modal("Abre el constructor de selección rápida.", true, "QSELECT"),
        "ribbon.herramientas.similar" => Tool("Selecciona objetos similares al indicado.", "SELECTSIMILAR"),
        "ribbon.herramientas.invertir" => Modal("Previsualiza la inversión de la selección actual.", true),
        "ribbon.herramientas.auditar" => Modal("Previsualiza la auditoría del dibujo.", true, "AUDIT"),
        "ribbon.herramientas.limpiar" => Modal("Abre Purgar / Renombrar para preparar la limpieza.", true, "PURGE"),
        "ribbon.herramientas.recuperar" => Modal("Prepara la recuperación de un dibujo.", true, "RECOVER"),
        "ribbon.herramientas.opciones" => Modal("Abre las opciones locales de ArcCAD.", false, "OP"),

        // Smart
        "ribbon.smart.asistente" => Modal("Abre el asistente Smart en modo de demostración.", true),
        "ribbon.smart.sugerir-comando" => Modal("Sugiere el siguiente comando según el contexto.", true),
        "ribbon.smart.explicar-seleccion" => Modal("Explica propiedades y relaciones de la selección.", true),
        "ribbon.smart.smart-select" => Modal("Configura criterios de selección inteligente.", true),
        "ribbon.smart.por-propiedades" => Modal("Agrupa y filtra objetos por propiedades.", true),
        "ribbon.smart.similar" => Modal("Busca objetos visualmente similares.", true),
        "ribbon.smart.agrupar" => Modal("Propone agrupaciones de objetos relacionadas.", true),
        "ribbon.smart.detectar-geometria" => Modal("Detecta patrones geométricos en el dibujo.", true),
        "ribbon.smart.muros" => Modal("Reconoce trazos candidatos a muros.", true),
        "ribbon.smart.aberturas" => Modal("Reconoce aberturas en recintos y muros.", true),
        "ribbon.smart.simbolos" => Modal("Reconoce símbolos repetidos y candidatos a bloque.", true),
        "ribbon.smart.limpiar-dibujo" => Modal("Propone una limpieza inteligente del dibujo.", true),
        "ribbon.smart.unir" => Modal("Propone uniones de geometría compatibles.", true),
        "ribbon.smart.duplicados" => Modal("Detecta objetos duplicados o superpuestos.", true),
        "ribbon.smart.simplificar" => Modal("Propone simplificaciones de geometría.", true),

        // Manage
        "ribbon.administra.propiedades" => Dock("Abre la instancia única del Administrador de capas.", false, "LA"),
        "ribbon.administra.estados" => Flyout("Abre los estados de capa guardados."),
        "ribbon.administra.aislar" => Toggle("Alterna el aislamiento de la capa seleccionada.", false, "LAYISO"),
        "ribbon.administra.combinar" => Modal("Configura la combinación de capas.", true, "LAYMRG"),
        "ribbon.administra.texto" => Modal("Abre el editor de estilos de texto.", true, "ST"),
        "ribbon.administra.cota" => Modal("Abre el editor de estilos de cota.", true, "D"),
        "ribbon.administra.tabla" => Modal("Abre el editor de estilos de tabla.", true, "TABLESTYLE"),
        "ribbon.administra.directriz" => Modal("Abre el editor de estilos de directriz.", true, "MLEADERSTYLE"),
        "ribbon.administra.comprobar" => Modal("Previsualiza la comprobación de estándares CAD.", true, "CHECKSTANDARDS"),
        "ribbon.administra.configurar" => Modal("Configura los estándares asociados al dibujo.", true),
        "ribbon.administra.traducir-capas" => Modal("Configura la traducción entre estándares de capas.", true, "LAYTRANS"),
        "ribbon.administra.actualizar" => Modal("Previsualiza la actualización de estándares.", true),
        "ribbon.administra.auditar" => Modal("Previsualiza la auditoría del dibujo.", true, "AUDIT"),
        "ribbon.administra.limpiar" => Modal("Abre Purgar / Renombrar para preparar la limpieza.", true, "PURGE"),
        "ribbon.administra.recuperar" => Modal("Prepara la recuperación de un dibujo.", true, "RECOVER"),
        "ribbon.administra.personalizar-iu" => Modal("Abre la personalización temporal de la interfaz."),

        // Export
        "ribbon.exportar.trazar" => Modal("Abre la configuración de Trazar / Publicar.", true, "Ctrl+P"),
        "ribbon.exportar.configurar-pagina" => Modal("Abre la configuración de página.", true, "PAGESETUP"),
        "ribbon.exportar.vista-previa" => Modal("Muestra una vista previa de la salida.", true, "PREVIEW"),
        "ribbon.exportar.publicar" => Modal("Configura la publicación de varias hojas.", true, "PUBLISH"),
        "ribbon.exportar.exportar-pdf" => Modal("Prepara la exportación a PDF.", true, "EXPORTPDF"),
        "ribbon.exportar.dwf" => Modal("Prepara la exportación a DWF.", true, "EXPORTDWF"),
        "ribbon.exportar.opciones" => Flyout("Abre las opciones específicas de PDF y DWF."),
        "ribbon.exportar.dwg" => Modal("Prepara una copia en formato DWG.", true),
        "ribbon.exportar.dxf" => Modal("Prepara una copia en formato DXF.", true),
        "ribbon.exportar.guardar-copia" => Modal("Configura una copia del dibujo sin guardarla.", true, "Ctrl+Shift+S"),
        "ribbon.exportar.version" => Flyout("Abre el selector de versión del formato de salida."),
        "ribbon.exportar.png-jpg" => Modal("Prepara una imagen PNG o JPG.", true),
        "ribbon.exportar.svg" => Modal("Prepara una salida vectorial SVG.", true),
        "ribbon.exportar.stl-obj" => Modal("Prepara una salida de modelo STL u OBJ.", true),
        "ribbon.exportar.compartir" => Unavailable("Comparte la salida cuando ArcCloud esté disponible."),

        // Online
        "ribbon.en-linea.abrir-nube" => Unavailable("Abre documentos de ArcCloud cuando exista conexión."),
        "ribbon.en-linea.guardar" => Unavailable("Guarda el documento en ArcCloud cuando exista conexión."),
        "ribbon.en-linea.sincronizar" => Unavailable("Sincroniza cambios cuando exista conexión."),
        "ribbon.en-linea.sin-conexion" => Toggle("Alterna el modo de trabajo sin conexión."),
        "ribbon.en-linea.compartir-vinculo" => Unavailable("Crea un vínculo compartido mediante ArcCloud."),
        "ribbon.en-linea.comentarios" => Modal("Abre comentarios de colaboración en estado sin conexión.", true),
        "ribbon.en-linea.permisos" => Modal("Muestra la configuración de permisos en modo de vista previa.", true),
        "ribbon.en-linea.avisos" => Flyout("Abre los avisos de colaboración.", true),
        "ribbon.en-linea.historial" => Unavailable("Consulta el historial remoto cuando exista conexión."),
        "ribbon.en-linea.comparar" => Unavailable("Compara versiones remotas cuando exista conexión."),
        "ribbon.en-linea.restaurar" => Unavailable("Restaura una versión remota cuando exista conexión."),
        "ribbon.en-linea.crear-version" => Unavailable("Crea una versión remota cuando exista conexión."),
        "ribbon.en-linea.actividad" => Unavailable("Consulta la actividad del proyecto cuando exista conexión."),
        "ribbon.en-linea.cuenta" => Modal("Muestra el acceso a la cuenta ArcCAD sin realizar solicitudes.", true),
        "ribbon.en-linea.ayuda" => Flyout("Abre la ayuda local de las funciones en línea.", false, "F1"),
        "ribbon.en-linea.plantillas" => Unavailable("Descarga plantillas cuando exista conexión."),

        // Geoservices
        "ribbon.geoservicio.definir-ubicacion" => Modal("Configura una ubicación geográfica en modo de vista previa.", true),
        "ribbon.geoservicio.buscar-direccion" => Unavailable("Busca direcciones cuando estén disponibles los geoservicios."),
        "ribbon.geoservicio.marcar-punto" => Modal("Prepara un marcador geográfico sin modificar el dibujo.", true),
        "ribbon.geoservicio.asignar-src" => Modal("Configura el sistema de referencia de coordenadas.", true),
        "ribbon.geoservicio.transformar" => Modal("Previsualiza una transformación de coordenadas.", true),
        "ribbon.geoservicio.unidades" => Flyout("Abre el selector de unidades geográficas."),
        "ribbon.geoservicio.mapa-base" => Flyout("Abre el selector de mapa base.", true),
        "ribbon.geoservicio.satelite" => Unavailable("Carga imágenes satelitales cuando exista conexión."),
        "ribbon.geoservicio.terreno" => Unavailable("Carga datos de terreno cuando exista conexión."),
        "ribbon.geoservicio.en-linea" => Toggle("Alterna el uso de mapas en línea.", true),
        "ribbon.geoservicio.capas-gis" => Modal("Administra capas GIS sin usar la paleta de capas CAD.", true),
        "ribbon.geoservicio.importar-shp" => Modal("Prepara la importación de datos SHP.", true),
        "ribbon.geoservicio.actualizar" => Unavailable("Actualiza fuentes GIS cuando exista conexión."),
        "ribbon.geoservicio.exportar-gis" => Modal("Prepara una exportación GIS.", true),

        _ => throw new InvalidOperationException($"No existe semántica de ribbon para '{actionId}'."),
    };

    private static RibbonCommandSemantics Immediate(string intent, string? shortcut = null) =>
        new(UiActionKind.Immediate, intent, Shortcut: shortcut);

    private static RibbonCommandSemantics Tool(string intent, string? shortcut = null) =>
        new(UiActionKind.Immediate, intent, Shortcut: shortcut, IsTool: true);

    private static RibbonCommandSemantics Toggle(string intent, bool backend = false, string? shortcut = null) =>
        new(UiActionKind.Toggle, intent, backend, shortcut);

    private static RibbonCommandSemantics Flyout(string intent, bool backend = false, string? shortcut = null) =>
        new(UiActionKind.Flyout, intent, backend, shortcut);

    private static RibbonCommandSemantics Dock(string intent, bool backend = false, string? shortcut = null) =>
        new(UiActionKind.Dock, intent, backend, shortcut);

    private static RibbonCommandSemantics Modal(string intent, bool backend = false, string? shortcut = null) =>
        new(UiActionKind.Modal, intent, backend, shortcut);

    private static RibbonCommandSemantics Unavailable(string intent, string? shortcut = null) =>
        new(UiActionKind.Unavailable, intent, true, shortcut);

    static RibbonCatalog()
    {
        Validate();
    }

    public static void Validate()
    {
        if (Sections.Count == 0)
        {
            throw new InvalidOperationException("El catálogo del ribbon no contiene secciones.");
        }

        if (AllCommands.Count != 139)
        {
            throw new InvalidOperationException($"Se esperaban 139 comandos contextuales y se encontraron {AllCommands.Count}.");
        }

        var actionIds = new HashSet<string>(StringComparer.Ordinal);
        foreach (var (sectionLabel, groups) in Sections)
        {
            if (string.IsNullOrWhiteSpace(sectionLabel) || groups is null || groups.Length == 0)
            {
                throw new InvalidOperationException("El catálogo del ribbon contiene una sección vacía.");
            }

            foreach (var group in groups)
            {
                if (string.IsNullOrWhiteSpace(group.Label))
                {
                    throw new InvalidOperationException($"La sección '{sectionLabel}' contiene un grupo sin etiqueta.");
                }

                if (group.Commands is null || group.Commands.Length == 0)
                {
                    throw new InvalidOperationException($"El grupo '{group.Label}' de '{sectionLabel}' no contiene comandos.");
                }

                foreach (var command in group.Commands)
                {
                    if (string.IsNullOrWhiteSpace(command.ActionId))
                    {
                        throw new InvalidOperationException($"El grupo '{group.Label}' contiene un comando sin ID.");
                    }

                    if (!actionIds.Add(command.ActionId))
                    {
                        throw new InvalidOperationException($"ID de acción duplicado: '{command.ActionId}'.");
                    }

                    if (string.IsNullOrWhiteSpace(command.Label) || string.IsNullOrWhiteSpace(command.Icon))
                    {
                        throw new InvalidOperationException($"La acción '{command.ActionId}' requiere etiqueta e icono.");
                    }

                    if (string.IsNullOrWhiteSpace(command.Intent))
                    {
                        throw new InvalidOperationException($"La acción '{command.ActionId}' requiere una intención semántica.");
                    }
                }
            }
        }
    }

    private static RibbonCommandDefinition C(string actionId, string label, string icon) =>
        new(actionId, label, icon);

    private static RibbonGroupDefinition G(
        string label,
        double width,
        params RibbonCommandDefinition[] commands) =>
        new(label, width, commands);
}
