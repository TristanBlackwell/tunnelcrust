#!/bin/bash

# A end-to-end test script which spins up the server, client, and mock server before
# sending a request all the way through the system.

# It's best to ensure you have built the server & client packages ahead of this script. This
# spawns each on a new thread and doesn't wait for build times so the request may fail if a
# component is not running when the request is made.

# Start components

echo "Starting server..."
(cd server && cargo run) &  # Run in the background
server_pid=$!

echo ""
echo ""
sleep 2


echo "Starting mock server..."
(cd mock-server && npm start) &
dummy_forward_pid=$!

echo ""
echo ""
sleep 2

echo "Starting client..."
(cd client && TUNNELCRUST_SERVER_URL=ws://localhost:5050 cargo r -- --port 8081) &
client_pid=$!


echo ""
echo ""
sleep 2

# Send a request to the server
echo "Sending test request..."
# This response should be from the mock server
response=$(curl -s http://localhost:5050)

echo ""
echo ""
sleep 2


echo "Response: $response"
if [[ "$response" == *"Upgrade required"* ]]; then
  echo "Test passed: received expected response."
else
  echo "Test failed: unexpected response."
fi

echo ""
echo ""
# Cleanup: Stop all processes
echo "Cleaning up..."
kill $server_pid
kill $dummy_forward_pid
kill $client_pid

# Wait a moment to ensure they terminate
wait $server_pid 2>/dev/null
wait $dummy_forward_pid 2>/dev/null
wait $client_pid 2>/dev/null

if lsof -i :8081 > /dev/null; then
  echo "Port 8081 is still in use; releasing..."
  lsof -ti :8081 | xargs kill -9
fi

echo ""
echo "All processes terminated."
