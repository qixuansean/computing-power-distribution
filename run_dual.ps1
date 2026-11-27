$proc1 = Start-Process -FilePath "C:\Users\qixua\Desktop\OCN\target\debug\ocn-node.exe" -ArgumentList "--node-id node-1 --listen 127.0.0.1:9200" -PassThru
Start-Sleep -Seconds 2
C:\Users\qixua\Desktop\OCN\target\debug\ocn-node.exe --node-id node-2 --listen 127.0.0.1:9201 --peer 127.0.0.1:9200
Stop-Process -Id $proc1.Id -Force
