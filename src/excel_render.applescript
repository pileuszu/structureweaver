-- Paths are process arguments, never interpolated into AppleScript source.
-- Only the uniquely named read-only copy belongs to this operation.
use framework "Foundation"
use scripting additions

on jsonText(value)
    set sourceString to current application's NSString's stringWithString:(value as text)
    set arrayValue to current application's NSArray's arrayWithObject:sourceString
    set encoded to current application's NSJSONSerialization's dataWithJSONObject:arrayValue options:0 |error|:(missing value)
    set resultText to (current application's NSString's alloc()'s initWithData:encoded encoding:(current application's NSUTF8StringEncoding)) as text
    return text 2 thru -2 of resultText
end jsonText

on jsonNumber(value)
    set numberValue to current application's NSNumber's numberWithDouble:(value as real)
    return (numberValue's stringValue()) as text
end jsonNumber

on writeText(value, destination)
    set sourceString to current application's NSString's stringWithString:(value as text)
    set succeeded to sourceString's writeToFile:destination atomically:true encoding:(current application's NSUTF8StringEncoding) |error|:(missing value)
    if not (succeeded as boolean) then error "Cannot write Excel recovery state"
end writeText

on recoverState(outputFolder)
    set statePath to outputFolder & "/recovery.json"
    set stateData to current application's NSData's dataWithContentsOfFile:statePath
    if stateData is missing value then return
    set stateObject to current application's NSJSONSerialization's JSONObjectWithData:stateData options:0 |error|:(missing value)
    if stateObject is missing value then error "Invalid Excel recovery state"
    set copyName to (stateObject's objectForKey:"copy_name") as text
    set previousName to (stateObject's objectForKey:"previous_name") as text
    try
        tell application id "com.microsoft.Excel"
            with timeout of 10 seconds
                -- Match only our generated copy. Never close the user's original.
                if exists workbook copyName then close workbook copyName saving no
                try
                    if previousName is not "" and (exists workbook previousName) then activate object workbook previousName
                end try
            end timeout
        end tell
    on error messageText number errorNumber
        error ("Excel 임시 통합 문서 닫기 단계 실패 (" & errorNumber & "): " & messageText) number errorNumber
    end try
    current application's NSFileManager's defaultManager()'s removeItemAtPath:statePath |error|:(missing value)
end recoverState

on recordObject(sheetName, objectName, objectKind, orderValue, geometry, rotationValue, fileName)
    return "{\"sheet_name\":" & my jsonText(sheetName) & ",\"object_name\":" & my jsonText(objectName) & ",\"kind\":" & my jsonText(objectKind) & ",\"z_order\":" & my jsonNumber(orderValue) & ",\"rect_points\":[" & my jsonNumber(item 1 of geometry) & "," & my jsonNumber(item 2 of geometry) & "," & my jsonNumber(item 3 of geometry) & "," & my jsonNumber(item 4 of geometry) & "],\"source_rotation_degrees\":" & my jsonNumber(rotationValue) & ",\"file\":" & my jsonText(fileName) & "}"
end recordObject

on run argv
    if (count argv) is not 3 then error "Expected mode, workbook copy, output folder"
    set operation to item 1 of argv
    set copyPath to item 2 of argv
    set outputFolder to item 3 of argv
    if operation is "recover" then
        my recoverState(outputFolder)
        return "recovered"
    end if
    set copyName to ((current application's NSString's stringWithString:copyPath)'s lastPathComponent()) as text
    set objectRecords to {}
    set warningRecords to {}
    set imageIndex to 0
    set startedAt to current date
    -- Querying active workbook is unsupported by some Mac Excel builds. It is
    -- unnecessary: closing only our copy naturally returns to the prior window.
    set previousName to ""
    set phaseText to "버전 조회"
    try
        tell application id "com.microsoft.Excel"
            with timeout of 10 seconds
                set excelVersion to version
            end timeout
        end tell
    on error messageText number errorNumber
        error ("Excel " & phaseText & " 단계 실패 (" & errorNumber & "): " & messageText) number errorNumber
    end try
    -- Rust supplied a macro-free static .xlsx with cached cell values. No
    -- application-wide Excel settings are queried or modified.
    my writeText("{\"copy_name\":" & my jsonText(copyName) & ",\"previous_name\":" & my jsonText(previousName) & "}", outputFolder & "/recovery.json")
    try
        tell application id "com.microsoft.Excel"
            with timeout of 10 seconds
                set phaseText to "정적 통합 문서 열기"
                set snapshotWorkbook to open workbook workbook file name ((POSIX file copyPath) as text) update links do not update links read only true ignore read only recommended true add to mru false
            end timeout
            set phaseText to "시트 목록 조회"
            with timeout of 10 seconds
                set worksheetCount to count worksheets of snapshotWorkbook
            end timeout
            repeat with sheetIndex from 1 to worksheetCount
                if (current date) - startedAt > 120 then error "Excel rendering time limit exceeded"
                with timeout of 10 seconds
                    set phaseText to "시트 이름 조회"
                    set currentSheet to worksheet sheetIndex of snapshotWorkbook
                    set sheetName to name of currentSheet
                    set phaseText to "도형 목록 조회: " & sheetName
                    set shapeCount to count shapes of currentSheet
                end timeout
                repeat with shapeIndex from 1 to shapeCount
                    if imageIndex >= 2048 then error "Excel drawing object limit exceeded"
                    if (current date) - startedAt > 120 then error "Excel rendering time limit exceeded"
                    set objectName to "object " & shapeIndex
                    set orderValue to -1
                    try
                        with timeout of 10 seconds
                            set phaseText to "도형 속성 조회: " & sheetName & " / " & shapeIndex
                            set currentShape to shape shapeIndex of currentSheet
                            set objectName to name of currentShape
                            if visible of currentShape then
                                set imageIndex to imageIndex + 1
                                set fileName to "object-" & imageIndex & ".png"
                                set objectKind to "shape"
                                if has chart of currentShape then set objectKind to "chart"
                                set geometry to {left position of currentShape, top of currentShape, width of currentShape, height of currentShape}
                                set orderValue to z order position of currentShape
                                set rotationValue to rotation of currentShape
                                set phaseText to "PNG 내보내기: " & sheetName & " / " & objectName
                                try
                                    save as picture currentShape picture type save as PNG file file name ((POSIX file (outputFolder & "/" & fileName)) as text)
                                on error
                                    if objectKind is "chart" then
                                        save as picture (chart object objectName of currentSheet) picture type save as PNG file file name ((POSIX file (outputFolder & "/" & fileName)) as text)
                                    else
                                        error "Excel could not export this shape"
                                    end if
                                end try
                                set end of objectRecords to my recordObject(sheetName, objectName, objectKind, orderValue, geometry, rotationValue, fileName)
                            end if
                        end timeout
                    on error messageText number errorNumber
                        if errorNumber is -1712 then error messageText number errorNumber
                        set end of warningRecords to my jsonText(phaseText & " (z=" & orderValue & ", 오류 " & errorNumber & "): " & messageText)
                    end try
                end repeat
            end repeat
            set phaseText to "차트 전용 시트 조회"
            with timeout of 10 seconds
                if (count chart sheets of snapshotWorkbook) > 0 then set end of warningRecords to my jsonText("차트 전용 시트는 현재 네이티브 렌더러로 표시합니다.")
            end timeout
        end tell
        set phaseText to "임시 통합 문서 정리"
        my recoverState(outputFolder)
    on error messageText number errorNumber
        set failureText to "Excel " & phaseText & " 단계 실패 (" & errorNumber & "): " & messageText
        try
            my recoverState(outputFolder)
        on error cleanupText
            set failureText to failureText & " / " & cleanupText
        end try
        error failureText number errorNumber
    end try
    set savedDelimiters to AppleScript's text item delimiters
    set AppleScript's text item delimiters to ","
    set objectJSON to objectRecords as text
    set warningsJSON to warningRecords as text
    set AppleScript's text item delimiters to savedDelimiters
    return "{\"excel_version\":" & my jsonText(excelVersion) & ",\"objects\":[" & objectJSON & "],\"warnings\":[" & warningsJSON & "]}"
end run
