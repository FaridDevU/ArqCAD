; ArcCAD .pat fixture with original patterns created for these tests.
; It does not copy acad.pat, zwcad.pat, or any third-party pattern library.

*LINE45,ArcCAD simple 45-degree hatch
45,0,0,0,.125

*CROSSHATCH,ArcForge cuadricula cruzada (dos familias)
0,0,0,0,.25
90,0,0,0,.25

*DOTGRID,ArcCAD dots with a dot-gap dash family
0,0,0,0,.125,0,-.125

; PARTIALLY_BROKEN has one nonnumeric family field. That line is skipped with a
; warning while the remaining valid families stay in the definition.
*PARTIALLY_BROKEN,ArcCAD pattern with one invalid family
0,0,0,0,.2
30,0,0,not-a-number,.1
60,0,0,0,.3

; EMPTY_DEF has no family line before the next header and is skipped with a warning.
*EMPTY_DEF,ArcCAD header without families

; Deliberate case-insensitive LINE45 redefinition tests last-definition wins.
*LINE45,ArcCAD 45-degree hatch redefined for the fixture
45,1,1,0,.2
