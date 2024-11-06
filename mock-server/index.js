const http = require('http');

const server = http.createServer((req, res) => {
  console.log("Request received");
  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('Back to you!\n');
});

const PORT = 8081;
server.listen(PORT, () => {
  console.log(`Server running at http://localhost:${PORT}/`);
});