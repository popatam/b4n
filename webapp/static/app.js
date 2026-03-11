$(function () {
  const pollIntervalMs = Number(document.body.dataset.pollInterval || 2000);
  const $chatFeed = $("#chat-feed");
  const $chatForm = $("#chat-form");
  const $messageInput = $("#message-input");
  const $sendButton = $("#send-button");
  const $statusLine = $("#status-line");
  const $nodeId = $("#node-id");
  const $confirmedCount = $("#confirmed-count");
  const $pendingCount = $("#pending-count");

  let refreshInFlight = false;

  function setStatus(text, isError) {
    $statusLine.text(text);
    $statusLine.toggleClass("status-error", Boolean(isError));
  }

  function buildMeta(message) {
    const parts = [
      `id ${message.id}`,
      `from ${message.from}`,
      message.status,
    ];

    if (message.block_index !== null && message.block_index !== undefined) {
      parts.push(`block ${message.block_index}`);
    }

    return parts.join(" · ");
  }

  function renderMessages(payload) {
    const shouldStickToBottom =
      $chatFeed[0].scrollHeight - $chatFeed.scrollTop() - $chatFeed.innerHeight() < 120;

    $nodeId.text(payload.node_id === null ? "?" : payload.node_id);
    $confirmedCount.text(payload.totals.confirmed);
    $pendingCount.text(payload.totals.pending);
    $chatFeed.empty();

    if (!payload.messages.length) {
      $chatFeed.append($("<div>", { class: "empty-state", text: "В цепочке и mempool пока нет сообщений." }));
      return;
    }

    payload.messages.forEach(function (message) {
      const isPending = message.status === "pending";
      const $card = $("<article>", {
        class: `message ${isPending ? "message--pending" : "message--confirmed"}`,
      });

      $("<div>", { class: "message__badge", text: message.status }).appendTo($card);
      $("<p>", { class: "message__text", text: message.text }).appendTo($card);
      $("<div>", { class: "message__meta", text: buildMeta(message) }).appendTo($card);
      $chatFeed.append($card);
    });

    if (shouldStickToBottom) {
      $chatFeed.scrollTop($chatFeed[0].scrollHeight);
    }
  }

  function showRequestError(xhr, fallbackText) {
    const detail = xhr.responseJSON && xhr.responseJSON.detail ? xhr.responseJSON.detail : fallbackText;
    setStatus(detail, true);
  }

  function refreshMessages() {
    if (refreshInFlight) {
      return;
    }

    refreshInFlight = true;
    $.getJSON("/api/messages")
      .done(function (payload) {
        renderMessages(payload);
        setStatus("Состояние синхронизировано.");
      })
      .fail(function (xhr) {
        showRequestError(xhr, "Не удалось прочитать состояние ноды.");
      })
      .always(function () {
        refreshInFlight = false;
      });
  }

  $chatForm.on("submit", function (event) {
    event.preventDefault();

    const text = $messageInput.val().trim();
    if (!text) {
      setStatus("Сообщение не должно быть пустым.", true);
      return;
    }

    $sendButton.prop("disabled", true);
    setStatus("Сообщение отправляется в ноду...");

    $.ajax({
      url: "/api/messages",
      method: "POST",
      contentType: "application/json; charset=utf-8",
      data: JSON.stringify({ text: text }),
    })
      .done(function (payload) {
        renderMessages(payload);
        $messageInput.val("");
        setStatus("Сообщение отправлено. Ожидаем подтверждение блоком.");
      })
      .fail(function (xhr) {
        showRequestError(xhr, "Не удалось отправить сообщение.");
      })
      .always(function () {
        $sendButton.prop("disabled", false);
      });
  });

  refreshMessages();
  window.setInterval(refreshMessages, pollIntervalMs);
});
