const http = require('http');
const port = process.env.PORT || 3000;
http
  .createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end('<h1>bunny example app</h1><p>Port ' + port + '</p>');
  })
  .listen(port, '127.0.0.1', () => console.log('Listening on', port));
