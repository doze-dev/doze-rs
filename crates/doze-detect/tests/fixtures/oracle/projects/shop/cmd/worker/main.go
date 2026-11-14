// worker is the shop fixture's background consumer. It reads orders from
// kafka and, for each one, queues a message on SQS and writes a receipt to S3.
// Consuming is what keeps kafka awake; the first order is what wakes aws.
package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"strings"
	"syscall"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
	"github.com/aws/aws-sdk-go-v2/service/sqs"
	"github.com/twmb/franz-go/pkg/kgo"
)

func main() {
	log := slog.New(slog.NewTextHandler(os.Stdout, nil))
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	events, err := kgo.NewClient(
		kgo.SeedBrokers(strings.Split(os.Getenv("KAFKA_BROKERS"), ",")...),
		kgo.ConsumerGroup("worker"),
		kgo.ConsumeTopics("orders"),
		kgo.AllowAutoTopicCreation(),
	)
	if err != nil {
		log.Error("kafka client", "err", err)
		os.Exit(1)
	}
	defer events.Close()

	// Local AWS: fixed credentials, and the endpoint doze hands us.
	cfg := aws.Config{
		Region:       "us-east-1",
		Credentials:  credentials.NewStaticCredentialsProvider("shop", "shop", ""),
		BaseEndpoint: aws.String(os.Getenv("AWS_ENDPOINT")),
	}
	queues := sqs.NewFromConfig(cfg)
	objects := s3.NewFromConfig(cfg, func(o *s3.Options) { o.UsePathStyle = true })
	queue, bucket := os.Getenv("ORDERS_QUEUE"), os.Getenv("UPLOADS")

	log.Info("consuming", "topic", "orders", "group", "worker")
	for {
		fetches := events.PollFetches(ctx)
		if ctx.Err() != nil {
			log.Info("stopping")
			return
		}
		fetches.EachError(func(topic string, partition int32, err error) {
			log.Warn("fetch", "topic", topic, "partition", partition, "err", err)
		})
		fetches.EachRecord(func(rec *kgo.Record) {
			id := string(rec.Key)
			log.Info("picked up", "order", id, "partition", rec.Partition, "offset", rec.Offset)

			url, err := queues.GetQueueUrl(ctx, &sqs.GetQueueUrlInput{QueueName: aws.String(queue)})
			if err != nil {
				log.Error("find queue", "queue", queue, "err", err)
				return
			}
			if _, err := queues.SendMessage(ctx, &sqs.SendMessageInput{
				QueueUrl:    url.QueueUrl,
				MessageBody: aws.String(string(rec.Value)),
			}); err != nil {
				log.Error("queue order", "order", id, "err", err)
				return
			}
			key := fmt.Sprintf("receipts/%s.json", id)
			if _, err := objects.PutObject(ctx, &s3.PutObjectInput{
				Bucket: aws.String(bucket),
				Key:    aws.String(key),
				Body:   strings.NewReader(string(rec.Value)),
			}); err != nil {
				log.Error("write receipt", "key", key, "err", err)
				return
			}
			log.Info("processed", "order", id, "receipt", key)
		})
	}
}
