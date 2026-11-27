cd "C:\Users\qixua\Desktop\OCN"
$result = cargo check -p ocn-core 2>&1
$lines = $result -split "`n" | Where-Object { $_ -match "Compiling|error:" }
$lines | Write-Output -FilePath "C:\Users\qixua\Desktop\OCN\check_output.txt"
if ($result -match "error:") {
    Write-Output "HAS ERRORS"
} else {
    Write-Output "NO ERRORS"
}